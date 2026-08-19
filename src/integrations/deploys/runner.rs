use std::{
    future::Future,
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    pin::Pin,
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use serde_json::json;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    sync::{Semaphore, oneshot, oneshot::error::TryRecvError},
    task::JoinHandle,
    time::Instant,
};
use zeroize::Zeroizing;

use crate::inventory::Machine;

use super::{
    DeployCatalog, DeployError, DeployResult, OpenBaoSigner, catalog::ResolvedDeploy, normalize,
};

const MAX_STDOUT_BYTES: usize = 512 * 1024;
const MAX_STDERR_BYTES: usize = 32 * 1024;
const SYSTEM_INFO_BEGIN: &[u8] = b"HOMELAB_SYSTEM_INFO_V1_BEGIN\n";
const SYSTEM_INFO_END: &[u8] = b"HOMELAB_SYSTEM_INFO_V1_END\n";

#[derive(Clone, Copy)]
enum FailureClassification {
    NotFound,
    PermissionDenied,
    AlreadyExists,
    Timeout,
    NonzeroExit,
    WaitFailed,
    InvalidData,
    SshHostKeyVerification,
    SshAuthentication,
    ConnectionUnavailable,
    ConnectionTimeout,
    NameResolution,
    RuntimeSetup,
    RemoteCommand,
    Other,
}

impl FailureClassification {
    const fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::PermissionDenied => "permission_denied",
            Self::AlreadyExists => "already_exists",
            Self::Timeout => "timeout",
            Self::NonzeroExit => "nonzero_exit",
            Self::WaitFailed => "wait_failed",
            Self::InvalidData => "invalid_data",
            Self::SshHostKeyVerification => "ssh_host_key_verification",
            Self::SshAuthentication => "ssh_authentication",
            Self::ConnectionUnavailable => "connection_unavailable",
            Self::ConnectionTimeout => "connection_timeout",
            Self::NameResolution => "name_resolution",
            Self::RuntimeSetup => "runtime_setup",
            Self::RemoteCommand => "remote_command",
            Self::Other => "other",
        }
    }
}

struct DeployDiagnostics {
    correlation_id: uuid::Uuid,
    deploy_id: String,
    machine_id: uuid::Uuid,
}

impl DeployDiagnostics {
    fn new(deploy_id: &str, machine_id: uuid::Uuid) -> Self {
        Self {
            correlation_id: uuid::Uuid::new_v4(),
            deploy_id: deploy_id.to_owned(),
            machine_id,
        }
    }

    fn failure(
        &self,
        stage: &'static str,
        classification: FailureClassification,
        exit_code: Option<i32>,
    ) {
        let correlation_id = self.correlation_id;
        let deploy_id = self.deploy_id.as_str();
        let machine_id = self.machine_id;
        let classification = classification.as_str();
        if let Some(exit_code) = exit_code {
            tracing::warn!(
                deploy.correlation_id = %correlation_id,
                deploy.id = deploy_id,
                machine.id = %machine_id,
                deploy.stage = stage,
                error.classification = classification,
                process.exit_code = exit_code,
                "deploy stage failed"
            );
        } else {
            tracing::warn!(
                deploy.correlation_id = %correlation_id,
                deploy.id = deploy_id,
                machine.id = %machine_id,
                deploy.stage = stage,
                error.classification = classification,
                "deploy stage failed"
            );
        }
    }
}

fn classify_io(error: &std::io::Error) -> FailureClassification {
    match error.kind() {
        std::io::ErrorKind::NotFound => FailureClassification::NotFound,
        std::io::ErrorKind::PermissionDenied => FailureClassification::PermissionDenied,
        std::io::ErrorKind::AlreadyExists => FailureClassification::AlreadyExists,
        std::io::ErrorKind::TimedOut => FailureClassification::Timeout,
        std::io::ErrorKind::InvalidData => FailureClassification::InvalidData,
        _ => FailureClassification::Other,
    }
}

fn classify_uv_stderr(stderr: &[u8]) -> FailureClassification {
    if contains_bytes(stderr, b"An exception occurred in:") {
        FailureClassification::RuntimeSetup
    } else if (contains_bytes(stderr, b"SSH host key error (Host key for ")
        && contains_bytes(stderr, b" does not match.)"))
        || contains_bytes(stderr, b" not found in known_hosts")
        || (contains_bytes(stderr, b"Host key for server '")
            && contains_bytes(stderr, b"' does not match: got '"))
    {
        FailureClassification::SshHostKeyVerification
    } else if contains_bytes(stderr, b"Authentication failed.")
        || contains_bytes(stderr, b"Authentication failed:")
    {
        FailureClassification::SshAuthentication
    } else if contains_bytes(stderr, b"Could not resolve hostname (") {
        FailureClassification::NameResolution
    } else if contains_bytes(stderr, b"Could not connect (timed out)")
        || contains_bytes(
            stderr,
            b"Key-exchange timed out waiting for key negotiation",
        )
    {
        FailureClassification::ConnectionTimeout
    } else if contains_bytes(stderr, b"Could not connect (Unable to connect to port ") {
        FailureClassification::ConnectionUnavailable
    } else {
        FailureClassification::Other
    }
}

fn emit_uv_exit_failure(
    diagnostics: &DeployDiagnostics,
    stderr: Option<&[u8]>,
    exit_code: Option<i32>,
) {
    let classification = if stderr.is_some_and(|stderr| {
        contains_bytes(stderr, SYSTEM_INFO_BEGIN) && !contains_bytes(stderr, SYSTEM_INFO_END)
    }) {
        FailureClassification::RemoteCommand
    } else {
        stderr
            .map(classify_uv_stderr)
            .unwrap_or(FailureClassification::WaitFailed)
    };
    diagnostics.failure("uv_exit", classification, exit_code);
}

fn contains_bytes(value: &[u8], pattern: &[u8]) -> bool {
    value.windows(pattern.len()).any(|window| window == pattern)
}

#[derive(Clone, Debug)]
pub struct DeployRunnerConfig {
    pub uv_executable: PathBuf,
    pub ssh_keygen_executable: PathBuf,
    pub temp_root: PathBuf,
}

#[derive(Clone)]
pub struct DeployRunner<S> {
    catalog: DeployCatalog,
    signer: Arc<S>,
    config: Arc<DeployRunnerConfig>,
    permit: Arc<Semaphore>,
}

impl<S: OpenBaoSigner> DeployRunner<S> {
    pub fn new(
        catalog: DeployCatalog,
        signer: S,
        config: DeployRunnerConfig,
    ) -> Result<Self, DeployError> {
        if !config.uv_executable.is_absolute()
            || !config.ssh_keygen_executable.is_absolute()
            || !config.temp_root.is_absolute()
        {
            return Err(DeployError::InvalidConfiguration);
        }
        let temp = config
            .temp_root
            .canonicalize()
            .map_err(|_| DeployError::InvalidConfiguration)?;
        if !temp.is_dir() {
            return Err(DeployError::InvalidConfiguration);
        }
        Ok(Self {
            catalog,
            signer: Arc::new(signer),
            config: Arc::new(DeployRunnerConfig {
                temp_root: temp,
                ..config
            }),
            permit: Arc::new(Semaphore::new(1)),
        })
    }

    pub fn list(&self) -> Vec<super::DeployMetadata> {
        self.catalog.list()
    }

    pub async fn run(
        &self,
        deploy_id: &str,
        machine: Machine,
    ) -> Result<DeployResult, DeployError> {
        let mut never = Box::pin(std::future::pending());
        self.run_cancelled(deploy_id, machine, never.as_mut()).await
    }

    pub async fn run_cancelled(
        &self,
        deploy_id: &str,
        machine: Machine,
        mut cancellation: Pin<&mut (dyn Future<Output = ()> + Send)>,
    ) -> Result<DeployResult, DeployError> {
        let diagnostics = DeployDiagnostics::new(deploy_id, machine.id);
        let permit = self
            .permit
            .clone()
            .try_acquire_owned()
            .map_err(|_| DeployError::Busy)?;
        let deploy = self.catalog.resolve(deploy_id)?;
        if machine.pinned_host_public_key.is_none() {
            return Err(DeployError::MissingHostPin);
        }
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let (result_tx, mut result_rx) = oneshot::channel();
        let signer = self.signer.clone();
        let config = self.config.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _ = result_tx
                .send(worker(deploy, machine, signer, config, cancel_rx, diagnostics).await);
        });
        tokio::select! {
            biased;
            result = &mut result_rx => { drop(cancel_tx); result.unwrap_or(Err(DeployError::ExecutionOutcomeUnknown)) }
            () = cancellation.as_mut() => {
                let _ = cancel_tx.send(());
                result_rx.await.unwrap_or(Err(DeployError::ExecutionOutcomeUnknown))
            }
        }
    }
}

async fn worker<S: OpenBaoSigner>(
    deploy: ResolvedDeploy,
    machine: Machine,
    signer: Arc<S>,
    config: Arc<DeployRunnerConfig>,
    mut cancelled: oneshot::Receiver<()>,
    diagnostics: DeployDiagnostics,
) -> Result<DeployResult, DeployError> {
    cancelled_now(&mut cancelled)?;
    let pin = machine
        .pinned_host_public_key
        .as_deref()
        .ok_or(DeployError::MissingHostPin)?;
    validate_host_pin(pin)?;
    let run_dir = create_run_dir(&config.temp_root, &diagnostics)?;
    let _cleanup = Cleanup(run_dir.clone());
    let key = run_dir.join("identity");
    let known_hosts = run_dir.join("known_hosts");
    run_keygen(
        &config.ssh_keygen_executable,
        &key,
        &mut cancelled,
        &diagnostics,
    )
    .await?;
    let public_key = read_bounded(&key.with_extension("pub"), 1024, &diagnostics)?;
    cancelled_now(&mut cancelled)?;
    let certificate = Zeroizing::new(tokio::select! {
        result = signer.sign(public_key.trim_end()) => result?,
        _ = &mut cancelled => return Err(DeployError::Cancelled),
    });
    write_private(
        &key.with_file_name("identity-cert.pub"),
        certificate.as_bytes(),
        0o600,
        "certificate_write",
        &diagnostics,
    )?;
    std::fs::remove_file(key.with_extension("pub")).map_err(|error| {
        diagnostics.failure("public_key_remove", classify_io(&error), None);
        DeployError::ExecutionRejected
    })?;
    write_private(
        &known_hosts,
        format!(
            "{} {}\n",
            known_hosts_host(&machine.ssh_host, machine.ssh_port),
            pin
        )
        .as_bytes(),
        0o600,
        "known_hosts_write",
        &diagnostics,
    )?;
    let inventory = serde_json::to_string(&json!({"version":1,"host":{"address":machine.ssh_host,"user":machine.ssh_username,"port":machine.ssh_port,"ssh_key":key,"known_hosts":known_hosts}}))
        .map_err(|_| {
            diagnostics.failure(
                "inventory_serialize",
                FailureClassification::InvalidData,
                None,
            );
            DeployError::ExecutionRejected
        })?;
    let output = run_uv(
        &config.uv_executable,
        &config.temp_root,
        &deploy,
        &inventory,
        &mut cancelled,
        &diagnostics,
    )
    .await?;
    normalize::system_info(&output)
}

async fn run_keygen(
    executable: &Path,
    key: &Path,
    cancelled: &mut oneshot::Receiver<()>,
    diagnostics: &DeployDiagnostics,
) -> Result<(), DeployError> {
    let mut command = Command::new(executable);
    command
        .env_clear()
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "", "-f"])
        .arg(key)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    process_group(&mut command);
    let mut child = command.spawn().map_err(|error| {
        diagnostics.failure("keygen_spawn", classify_io(&error), None);
        DeployError::ExecutionRejected
    })?;
    let result = tokio::select! {
        result = child.wait() => match result {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => {
                diagnostics.failure("keygen_exit", FailureClassification::NonzeroExit, status.code());
                Err(DeployError::ExecutionRejected)
            }
            Err(_) => {
                diagnostics.failure("keygen_exit", FailureClassification::WaitFailed, None);
                Err(DeployError::ExecutionRejected)
            }
        },
        _ = &mut *cancelled => { terminate_group(&mut child, None, None).await; Err(DeployError::Cancelled) },
        _ = tokio::time::sleep(Duration::from_secs(10)) => {
            terminate_group(&mut child, None, None).await;
            diagnostics.failure("keygen_timeout", FailureClassification::Timeout, None);
            Err(DeployError::ExecutionRejected)
        }
    };
    result?;
    for path in [key.to_path_buf(), key.with_extension("pub")] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(
            |error| {
                diagnostics.failure("key_chmod", classify_io(&error), None);
                DeployError::ExecutionRejected
            },
        )?;
    }
    Ok(())
}

async fn run_uv(
    executable: &Path,
    temp_root: &Path,
    deploy: &ResolvedDeploy,
    inventory: &str,
    cancelled: &mut oneshot::Receiver<()>,
    diagnostics: &DeployDiagnostics,
) -> Result<Vec<u8>, DeployError> {
    let project_environment = deploy.root.join(".venv");
    let path = format!(
        "{}:/usr/local/bin:/usr/bin:/bin",
        project_environment.join("bin").display()
    );
    let mut command = Command::new(executable);
    command
        .env_clear()
        .env("HOME", temp_root)
        .env("HOMELAB_INVENTORY_JSON", inventory)
        .env("PATH", path)
        .env("UV_NO_CACHE", "1")
        .env("UV_NO_SYNC", "1")
        .env("UV_OFFLINE", "1")
        .env("UV_PROJECT_ENVIRONMENT", project_environment)
        .args(["run", "--locked", "pyinfra", "--yes"])
        .arg(&deploy.inventory)
        .arg(&deploy.entrypoint)
        .current_dir(&deploy.root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    process_group(&mut command);
    let mut child = command.spawn().map_err(|error| {
        diagnostics.failure("uv_spawn", classify_io(&error), None);
        DeployError::ExecutionRejected
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or(DeployError::ExecutionOutcomeUnknown)?;
    let stderr = child
        .stderr
        .take()
        .ok_or(DeployError::ExecutionOutcomeUnknown)?;
    let mut stdout_task = Some(tokio::spawn(drain(stdout, MAX_STDOUT_BYTES)));
    let mut stderr_task = Some(tokio::spawn(drain_sensitive(stderr, MAX_STDERR_BYTES)));
    let expires = Instant::now() + Duration::from_secs(deploy.timeout_seconds);
    let mut status: Option<std::process::ExitStatus> = None;
    let mut output: Option<Vec<u8>> = None;
    let mut stderr_output: Option<Zeroizing<Vec<u8>>> = None;
    loop {
        if let (Some(status), Some(output), Some(stderr)) =
            (status.as_ref(), output.as_ref(), stderr_output.as_ref())
        {
            return if status.success() {
                Ok(output.clone())
            } else {
                emit_uv_exit_failure(diagnostics, Some(stderr), status.code());
                Err(DeployError::ExecutionOutcomeUnknown)
            };
        }
        tokio::select! {
            biased;
            _ = &mut *cancelled => { terminate_group(&mut child, stdout_task.take(), stderr_task.take()).await; return Err(DeployError::CancelledOutcomeUnknown); }
            _ = tokio::time::sleep_until(expires) => { terminate_group(&mut child, stdout_task.take(), stderr_task.take()).await; return Err(DeployError::TimeoutOutcomeUnknown); }
            result = child.wait(), if status.is_none() => match result { Ok(value) => status = Some(value), Err(_) => { emit_uv_exit_failure(diagnostics, None, None); terminate_group(&mut child, stdout_task.take(), stderr_task.take()).await; return Err(DeployError::ExecutionOutcomeUnknown); } },
            result = async { stdout_task.as_mut().expect("guarded").await }, if stdout_task.is_some() => { stdout_task = None; match result { Ok(Ok(bytes)) => output = Some(bytes), _ => { terminate_group(&mut child, None, stderr_task.take()).await; return Err(DeployError::OutputTooLargeOutcomeUnknown); } } },
            result = async { stderr_task.as_mut().expect("guarded").await }, if stderr_task.is_some() => { stderr_task = None; match result { Ok(Ok(bytes)) => stderr_output = Some(bytes), _ => { terminate_group(&mut child, stdout_task.take(), None).await; return Err(DeployError::OutputTooLargeOutcomeUnknown); } } },
        }
    }
}

fn process_group(command: &mut Command) {
    unsafe {
        command.as_std_mut().pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

async fn terminate_group(
    child: &mut Child,
    stdout: Option<JoinHandle<Result<Vec<u8>, ()>>>,
    stderr: Option<JoinHandle<Result<Zeroizing<Vec<u8>>, ()>>>,
) {
    if let Some(pid) = child.id() {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    let _ = child.wait().await;
    if let Some(task) = stdout {
        task.abort();
        let _ = task.await;
    }
    if let Some(task) = stderr {
        task.abort();
        let _ = task.await;
    }
}

async fn drain(mut stream: impl AsyncRead + Unpin, limit: usize) -> Result<Vec<u8>, ()> {
    let mut output = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let read = stream.read(&mut buffer).await.map_err(|_| ())?;
        if read == 0 {
            return Ok(output);
        }
        if read > limit.saturating_sub(output.len()) {
            return Err(());
        }
        output.extend_from_slice(&buffer[..read]);
    }
}

async fn drain_sensitive(
    mut stream: impl AsyncRead + Unpin,
    limit: usize,
) -> Result<Zeroizing<Vec<u8>>, ()> {
    let mut output = Zeroizing::new(Vec::with_capacity(limit));
    let mut buffer = Zeroizing::new([0; 8192]);
    loop {
        let read = stream.read(&mut *buffer).await.map_err(|_| ())?;
        if read == 0 {
            return Ok(output);
        }
        if read > limit.saturating_sub(output.len()) {
            return Err(());
        }
        output.extend_from_slice(&buffer[..read]);
        buffer[..read].fill(0);
    }
}

fn create_run_dir(root: &Path, diagnostics: &DeployDiagnostics) -> Result<PathBuf, DeployError> {
    for _ in 0..8 {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|_| {
            diagnostics.failure("run_dir_create", FailureClassification::Other, None);
            DeployError::ExecutionRejected
        })?;
        let name = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let path = root.join(format!("homelab-deploy-{name}"));
        match std::fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => {
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                diagnostics.failure("run_dir_create", classify_io(&error), None);
                return Err(DeployError::ExecutionRejected);
            }
        }
    }
    diagnostics.failure("run_dir_create", FailureClassification::AlreadyExists, None);
    Err(DeployError::ExecutionRejected)
}

fn write_private(
    path: &Path,
    bytes: &[u8],
    mode: u32,
    stage: &'static str,
    diagnostics: &DeployDiagnostics,
) -> Result<(), DeployError> {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .map_err(|error| {
            diagnostics.failure(stage, classify_io(&error), None);
            DeployError::ExecutionRejected
        })?;
    file.write_all(bytes).map_err(|error| {
        diagnostics.failure(stage, classify_io(&error), None);
        DeployError::ExecutionRejected
    })
}

fn read_bounded(
    path: &Path,
    limit: u64,
    diagnostics: &DeployDiagnostics,
) -> Result<String, DeployError> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        diagnostics.failure("key_file_read", classify_io(&error), None);
        DeployError::ExecutionRejected
    })?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        diagnostics.failure("key_file_read", FailureClassification::InvalidData, None);
        return Err(DeployError::CredentialInvalid);
    }
    std::fs::read_to_string(path).map_err(|error| {
        diagnostics.failure("key_file_read", classify_io(&error), None);
        DeployError::CredentialInvalid
    })
}

fn known_hosts_host(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_owned()
    } else {
        format!("[{host}]:{port}")
    }
}

fn validate_host_pin(pin: &str) -> Result<(), DeployError> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let mut fields = pin.split(' ');
    if fields.next() != Some("ssh-ed25519") {
        return Err(DeployError::MissingHostPin);
    }
    let encoded = fields.next().ok_or(DeployError::MissingHostPin)?;
    if encoded.is_empty() || fields.next().is_some() || pin.len() > 256 {
        return Err(DeployError::MissingHostPin);
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| DeployError::MissingHostPin)?;
    if bytes.len() != 51
        || &bytes[..15] != b"\0\0\0\x0bssh-ed25519"
        || &bytes[15..19] != b"\0\0\0\x20"
    {
        return Err(DeployError::MissingHostPin);
    }
    Ok(())
}

fn cancelled_now(cancelled: &mut oneshot::Receiver<()>) -> Result<(), DeployError> {
    match cancelled.try_recv() {
        Ok(()) | Err(TryRecvError::Closed) => Err(DeployError::Cancelled),
        Err(TryRecvError::Empty) => Ok(()),
    }
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::{
        collections::BTreeMap,
        fmt, fs,
        os::unix::fs::PermissionsExt,
        sync::{
            Mutex,
            atomic::{AtomicU64, Ordering},
        },
    };
    use tracing::{Event, Subscriber, field::Visit};
    use tracing_subscriber::{Layer, layer::Context, layer::SubscriberExt as _};
    use uuid::Uuid;

    const PIN: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[derive(Clone)]
    struct Signer;
    impl OpenBaoSigner for Signer {
        fn sign<'a>(
            &'a self,
            public_key: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<String, DeployError>> + Send + 'a>> {
            assert_eq!(public_key, "ssh-ed25519 AAAATEST");
            Box::pin(async { Ok("ssh-ed25519-cert-v01@openssh.com AAACERT".into()) })
        }
    }

    #[derive(Clone)]
    struct PublicKeyRemovalFailureSigner {
        temp_root: PathBuf,
    }

    impl OpenBaoSigner for PublicKeyRemovalFailureSigner {
        fn sign<'a>(
            &'a self,
            public_key: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<String, DeployError>> + Send + 'a>> {
            assert_eq!(public_key, "ssh-ed25519 AAAATEST");
            let mut run_dirs = fs::read_dir(&self.temp_root).unwrap();
            let public_key_path = run_dirs
                .next()
                .unwrap()
                .unwrap()
                .path()
                .join("identity.pub");
            assert!(run_dirs.next().is_none());
            fs::remove_file(&public_key_path).unwrap();
            fs::create_dir(&public_key_path).unwrap();
            Box::pin(async { Ok("ssh-ed25519-cert-v01@openssh.com AAACERT".into()) })
        }
    }

    fn temporary() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "homelab-deploy-runner-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn executable(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn output() -> String {
        let mut text = "HOMELAB_SYSTEM_INFO_V1_BEGIN\n".to_owned();
        for section in [
            "hostname",
            "uptime",
            "boot_time",
            "os_release",
            "kernel_arch",
            "cpu",
            "memory",
            "filesystems",
            "block_devices",
            "interfaces",
            "default_routes",
        ] {
            text.push_str(&format!(
                "--- {section} BEGIN ---\nsafe\n--- {section} END ---\n"
            ));
        }
        text.push_str("HOMELAB_SYSTEM_INFO_V1_END\n");
        text
    }

    fn setup(uv_body: &str, timeout: u64) -> (DeployRunner<Signer>, PathBuf) {
        let base = temporary();
        let root = base.join("root");
        let temp = base.join("temp");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&temp).unwrap();
        fs::write(root.join("inventory.py"), "").unwrap();
        fs::write(root.join("entrypoint.py"), "").unwrap();
        fs::write(root.join("catalog.json"), serde_json::to_vec(&json!({"version":1,"deploys":[{
            "id":"system-info","description":"test","entrypoint":"entrypoint.py","inventory":"inventory.py","risk":"low",
            "local_available":true,"mcp_available":true,"supported_distributions":["debian"],"timeout_seconds":timeout,
            "requires_sudo":false,"mutating":false
        }]})).unwrap()).unwrap();
        let keygen = base.join("keygen");
        executable(
            &keygen,
            "#!/bin/sh\nfor path do :; done\nprintf private > \"$path\"\nprintf 'ssh-ed25519 AAAATEST\\n' > \"$path.pub\"\n",
        );
        let uv = base.join("uv");
        executable(&uv, uv_body);
        let catalog = DeployCatalog::load(root.clone(), root.join("catalog.json")).unwrap();
        let runner = DeployRunner::new(
            catalog,
            Signer,
            DeployRunnerConfig {
                uv_executable: uv,
                ssh_keygen_executable: keygen,
                temp_root: temp,
            },
        )
        .unwrap();
        (runner, base)
    }

    fn machine(host: &str, port: u16, pin: Option<&str>) -> Machine {
        Machine {
            id: Uuid::new_v4(),
            display_name: "test".into(),
            ssh_host: host.into(),
            ssh_port: port,
            ssh_username: "homelab".into(),
            pinned_host_public_key: pin.map(str::to_owned),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    async fn captured_worker<S: OpenBaoSigner>(
        runner: &DeployRunner<S>,
        machine: Machine,
        config: Arc<DeployRunnerConfig>,
    ) -> (Result<DeployResult, DeployError>, Vec<CapturedEvent>) {
        let (_cancel, cancelled) = oneshot::channel();
        captured_worker_with_cancellation(runner, machine, config, cancelled).await
    }

    async fn captured_worker_with_cancellation<S: OpenBaoSigner>(
        runner: &DeployRunner<S>,
        machine: Machine,
        config: Arc<DeployRunnerConfig>,
        cancelled: oneshot::Receiver<()>,
    ) -> (Result<DeployResult, DeployError>, Vec<CapturedEvent>) {
        let machine_id = machine.id;
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(EventCapture(events.clone()));
        let _subscriber = tracing::subscriber::set_default(subscriber);
        let result = worker(
            runner.catalog.resolve("system-info").unwrap(),
            machine,
            runner.signer.clone(),
            config,
            cancelled,
            DeployDiagnostics::new("system-info", machine_id),
        )
        .await;
        drop(_subscriber);
        let events = Arc::try_unwrap(events).unwrap().into_inner().unwrap();
        (result, events)
    }

    fn assert_failure_event(
        event: &CapturedEvent,
        machine_id: Uuid,
        stage: &str,
        classification: &str,
        exit_code: Option<i64>,
    ) {
        assert_eq!(event.level, tracing::Level::WARN);
        assert_eq!(event.target, "homelab_mcp::integrations::deploys::runner");
        assert_eq!(event.fields["message"], "deploy stage failed");
        assert_eq!(event.fields["deploy.id"], "system-info");
        assert_eq!(event.fields["machine.id"], machine_id.to_string());
        assert_eq!(event.fields["deploy.stage"], stage);
        assert_eq!(event.fields["error.classification"], classification);
        let correlation_id = Uuid::parse_str(
            event.fields["deploy.correlation_id"]
                .as_str()
                .expect("correlation ID must be a string"),
        )
        .unwrap();
        assert_eq!(correlation_id.get_version_num(), 4);
        assert_eq!(
            event
                .fields
                .get("process.exit_code")
                .and_then(serde_json::Value::as_i64),
            exit_code,
        );
        assert_eq!(event.fields.len(), if exit_code.is_some() { 7 } else { 6 });
    }

    fn assert_event_excludes(event: &CapturedEvent, sentinels: &[&str]) {
        let captured = event
            .fields
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .chain(std::iter::once(event.target.clone()))
            .collect::<Vec<_>>()
            .join("\n");
        for sentinel in sentinels {
            assert!(!captured.contains(sentinel), "event exposed sentinel data");
        }
        for forbidden in [
            "error",
            "path",
            "host",
            "username",
            "inventory",
            "key",
            "certificate",
            "token",
            "environment",
            "command",
            "stdout",
            "stderr",
            "output",
        ] {
            assert!(
                !event.fields.contains_key(forbidden),
                "event exposed forbidden field {forbidden}"
            );
        }
    }

    #[test]
    fn known_hosts_formats_default_nondefault_and_ipv6() {
        assert_eq!(known_hosts_host("host.example", 22), "host.example");
        assert_eq!(
            known_hosts_host("host.example", 2222),
            "[host.example]:2222"
        );
        assert_eq!(known_hosts_host("2001:db8::1", 22), "2001:db8::1");
        assert_eq!(known_hosts_host("2001:db8::1", 2222), "[2001:db8::1]:2222");
    }

    #[tokio::test]
    async fn successful_helper_gets_fixed_invocation_strict_inventory_and_cleanup() {
        assert!(std::env::var_os("USER").is_some());
        let audit = temporary().join("audit");
        let script = format!(
            r#"#!/usr/bin/python3
import json, os, pathlib, sys
assert sys.argv[1:5] == ["run", "--locked", "pyinfra", "--yes"]
assert len(sys.argv) == 7 and sys.argv[5:] == ["inventory.py", "entrypoint.py"]
root = pathlib.Path.cwd()
expected = {{
    "HOME": str(root.parent / "temp"),
    "PATH": f"{{root / '.venv'}}/bin:/usr/local/bin:/usr/bin:/bin",
    "UV_OFFLINE": "1",
    "UV_NO_SYNC": "1",
    "UV_NO_CACHE": "1",
    "UV_PROJECT_ENVIRONMENT": str(root / ".venv"),
}}
assert all(os.environ.get(name) == value for name, value in expected.items())
assert "HOMELAB_INVENTORY_JSON" in os.environ
assert "USER" not in os.environ
value=json.loads(os.environ["HOMELAB_INVENTORY_JSON"]); host=value["host"]
assert set(value) == {{"version", "host"}} and set(host) == {{"address","user","port","ssh_key","known_hosts"}}
key = pathlib.Path(host["ssh_key"])
assert key.read_text() == "private"
assert pathlib.Path(str(key) + "-cert.pub").read_text() == "ssh-ed25519-cert-v01@openssh.com AAACERT"
assert not pathlib.Path(str(key) + ".pub").exists()
assert pathlib.Path(host["known_hosts"]).read_text() == "[2001:db8::1]:2222 {PIN}\n"
pathlib.Path("{audit}").write_text(str(pathlib.Path(host["ssh_key"]).parent))
print({output:?})
"#,
            audit = audit.display(),
            output = output()
        );
        let (runner, base) = setup(&script, 3);
        let result = runner
            .run("system-info", machine("2001:db8::1", 2222, Some(PIN)))
            .await
            .unwrap();
        let DeployResult::SystemInfo(info) = result;
        assert_eq!(info.sections["hostname"], "safe");
        let run_dir = PathBuf::from(fs::read_to_string(&audit).unwrap());
        assert!(!run_dir.exists());
        assert_eq!(fs::read_dir(base.join("temp")).unwrap().count(), 0);
        fs::remove_dir_all(base).unwrap();
        fs::remove_dir_all(audit.parent().unwrap()).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn public_key_removal_failure_rejects_before_uv_and_cleans_run_directory() {
        let (runner, base) = setup("#!/bin/sh\nprintf called > uv-audit\n", 2);
        let runner = DeployRunner {
            catalog: runner.catalog.clone(),
            signer: Arc::new(PublicKeyRemovalFailureSigner {
                temp_root: base.join("temp"),
            }),
            config: runner.config.clone(),
            permit: Arc::new(Semaphore::new(1)),
        };
        let machine = machine("SENTINEL_HOST", 22, Some(PIN));
        let machine_id = machine.id;

        let (result, events) = captured_worker(&runner, machine, runner.config.clone()).await;

        assert_eq!(result.unwrap_err(), DeployError::ExecutionRejected);
        assert_eq!(events.len(), 1);
        assert_failure_event(&events[0], machine_id, "public_key_remove", "other", None);
        assert_event_excludes(
            &events[0],
            &[
                "SENTINEL_HOST",
                "AAAATEST",
                "AAACERT",
                base.to_str().unwrap(),
            ],
        );
        assert!(!base.join("root/uv-audit").exists());
        assert_eq!(fs::read_dir(base.join("temp")).unwrap().count(), 0);
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn denies_unpinned_machine_before_any_helper_runs() {
        let (runner, base) = setup("#!/bin/sh\nexit 99\n", 2);
        assert_eq!(
            runner
                .run("system-info", machine("host", 22, None))
                .await
                .unwrap_err(),
            DeployError::MissingHostPin
        );
        assert_eq!(fs::read_dir(base.join("temp")).unwrap().count(), 0);
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn denies_malformed_pin_before_credential_generation_or_signing() {
        let (runner, base) = setup("#!/bin/sh\nexit 99\n", 2);
        let audit = base.join("keygen-audit");
        executable(
            &runner.config.ssh_keygen_executable,
            &format!("#!/bin/sh\nprintf called > {}\n", audit.display()),
        );
        assert_eq!(
            runner
                .run(
                    "system-info",
                    machine("host", 22, Some("ssh-ed25519 malformed")),
                )
                .await
                .unwrap_err(),
            DeployError::MissingHostPin
        );
        assert!(!audit.exists());
        assert_eq!(fs::read_dir(base.join("temp")).unwrap().count(), 0);
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn keygen_permission_failure_emits_safe_classified_event() {
        let (runner, base) = setup("#!/bin/sh\nexit 0\n", 2);
        fs::set_permissions(
            &runner.config.ssh_keygen_executable,
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let machine = machine("SENTINEL_HOST", 22, Some(PIN));
        let machine_id = machine.id;
        let (result, events) = captured_worker(&runner, machine, runner.config.clone()).await;

        let error = result.unwrap_err();
        assert_eq!(error, DeployError::ExecutionRejected);
        assert_eq!(error.code(), "execution_rejected");
        assert_eq!(events.len(), 1);
        assert_failure_event(
            &events[0],
            machine_id,
            "keygen_spawn",
            "permission_denied",
            None,
        );
        assert_event_excludes(&events[0], &["SENTINEL_HOST"]);
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn keygen_nonzero_exit_emits_code_without_child_output() {
        let (runner, base) = setup("#!/bin/sh\nexit 0\n", 2);
        executable(
            &runner.config.ssh_keygen_executable,
            "#!/bin/sh\nprintf SENTINEL_CHILD_OUTPUT >&2\nexit 7\n",
        );
        let machine = machine("SENTINEL_HOST", 22, Some(PIN));
        let machine_id = machine.id;
        let (result, events) = captured_worker(&runner, machine, runner.config.clone()).await;

        assert_eq!(result.unwrap_err(), DeployError::ExecutionRejected);
        assert_eq!(events.len(), 1);
        assert_failure_event(
            &events[0],
            machine_id,
            "keygen_exit",
            "nonzero_exit",
            Some(7),
        );
        assert_event_excludes(&events[0], &["SENTINEL_HOST", "SENTINEL_CHILD_OUTPUT"]);
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uv_not_found_emits_safe_event_and_preserves_public_error() {
        let (runner, base) = setup("#!/bin/sh\nexit 0\n", 2);
        let missing_uv = base.join("SENTINEL_PRIVATE_PATH");
        let config = Arc::new(DeployRunnerConfig {
            uv_executable: missing_uv,
            ..runner.config.as_ref().clone()
        });
        let mut machine = machine("SENTINEL_HOST", 22, Some(PIN));
        machine.ssh_username = "SENTINEL_TOKEN_USERNAME".into();
        let machine_id = machine.id;
        let (result, events) = captured_worker(&runner, machine, config).await;

        assert_eq!(result.unwrap_err(), DeployError::ExecutionRejected);
        assert_eq!(events.len(), 1);
        assert_failure_event(&events[0], machine_id, "uv_spawn", "not_found", None);
        assert_event_excludes(
            &events[0],
            &[
                "SENTINEL_PRIVATE_PATH",
                "SENTINEL_HOST",
                "SENTINEL_TOKEN_USERNAME",
                "AAAATEST",
                "AAACERT",
                PIN,
            ],
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uv_nonzero_exit_emits_each_allowlisted_safe_classification() {
        for (stderr, classification) in [
            (
                "SSH host key error (Host key for SENTINEL_HOST does not match.)",
                "ssh_host_key_verification",
            ),
            (
                "Host key for server 'SENTINEL_HOST' does not match: got 'SENTINEL_KEY', expected 'x'",
                "ssh_host_key_verification",
            ),
            (
                "Authentication failed. SENTINEL_TOKEN",
                "ssh_authentication",
            ),
            (
                "Could not connect (Unable to connect to port 22 on SENTINEL_HOST)",
                "connection_unavailable",
            ),
            (
                "Key-exchange timed out waiting for key negotiation SENTINEL_HOST",
                "connection_timeout",
            ),
            (
                "Could not resolve hostname (SENTINEL_HOST)",
                "name_resolution",
            ),
            (
                "An exception occurred in: SENTINEL_PRIVATE_PATH",
                "runtime_setup",
            ),
        ] {
            let script = format!("#!/bin/sh\nprintf '%s' {stderr:?} >&2\nexit 23\n");
            let (runner, base) = setup(&script, 2);
            let machine = machine("SENTINEL_HOST", 22, Some(PIN));
            let machine_id = machine.id;
            let (result, events) = captured_worker(&runner, machine, runner.config.clone()).await;

            assert_eq!(result.unwrap_err(), DeployError::ExecutionOutcomeUnknown);
            assert_eq!(events.len(), 1);
            assert_failure_event(&events[0], machine_id, "uv_exit", classification, Some(23));
            assert_event_excludes(
                &events[0],
                &[
                    "SENTINEL_HOST",
                    "SENTINEL_KEY",
                    "SENTINEL_TOKEN",
                    "SENTINEL_PRIVATE_PATH",
                ],
            );
            fs::remove_dir_all(base).unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uv_partial_system_info_protocol_is_remote_command_and_remains_secret() {
        let (runner, base) = setup(
            "#!/bin/sh\nprintf 'SENTINEL_STDOUT'\nprintf 'HOMELAB_SYSTEM_INFO_V1_BEGIN\\nSENTINEL_STDERR Authentication failed.' >&2\nexit 25\n",
            2,
        );
        let machine = machine("SENTINEL_HOST", 22, Some(PIN));
        let machine_id = machine.id;
        let (result, events) = captured_worker(&runner, machine, runner.config.clone()).await;

        assert_eq!(result.unwrap_err(), DeployError::ExecutionOutcomeUnknown);
        assert_eq!(events.len(), 1);
        assert_failure_event(
            &events[0],
            machine_id,
            "uv_exit",
            "remote_command",
            Some(25),
        );
        assert_event_excludes(
            &events[0],
            &["SENTINEL_HOST", "SENTINEL_STDOUT", "SENTINEL_STDERR"],
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uv_remote_command_detection_falls_back_for_other_protocol_states() {
        for stderr in [
            "Authentication failed. SENTINEL_STDERR",
            "HOMELAB_SYSTEM_INFO_V1_BEGIN\\nSENTINEL_STDERR\\nHOMELAB_SYSTEM_INFO_V1_END\\nAuthentication failed.",
        ] {
            let script = format!(
                "#!/bin/sh\nprintf 'SENTINEL_STDOUT'\nprintf '%b' {stderr:?} >&2\nexit 26\n"
            );
            let (runner, base) = setup(&script, 2);
            let machine = machine("SENTINEL_HOST", 22, Some(PIN));
            let machine_id = machine.id;
            let (result, events) = captured_worker(&runner, machine, runner.config.clone()).await;

            assert_eq!(result.unwrap_err(), DeployError::ExecutionOutcomeUnknown);
            assert_eq!(events.len(), 1);
            assert_failure_event(
                &events[0],
                machine_id,
                "uv_exit",
                "ssh_authentication",
                Some(26),
            );
            assert_event_excludes(
                &events[0],
                &["SENTINEL_HOST", "SENTINEL_STDOUT", "SENTINEL_STDERR"],
            );
            fs::remove_dir_all(base).unwrap();
        }
    }

    #[test]
    fn uv_runtime_setup_marker_takes_precedence_over_nested_failure_text() {
        assert_eq!(
            classify_uv_stderr(
                b"An exception occurred in: SENTINEL_PATH Authentication failed. Could not connect (timed out)"
            )
            .as_str(),
            "runtime_setup"
        );
    }

    #[tokio::test]
    async fn sensitive_drain_preallocates_the_complete_bound() {
        let bytes = b"SENTINEL_SENSITIVE_STDERR".to_vec();
        let output = drain_sensitive(std::io::Cursor::new(bytes.clone()), 1024)
            .await
            .unwrap();

        assert_eq!(output.as_slice(), bytes);
        assert_eq!(output.capacity(), 1024);
    }

    #[test]
    fn uv_wait_failure_emits_exact_safe_event_without_exit_code() {
        let machine_id = Uuid::new_v4();
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(EventCapture(events.clone()));
        tracing::subscriber::with_default(subscriber, || {
            emit_uv_exit_failure(
                &DeployDiagnostics::new("system-info", machine_id),
                None,
                None,
            );
        });
        let events = Arc::try_unwrap(events).unwrap().into_inner().unwrap();

        assert_eq!(events.len(), 1);
        assert_failure_event(&events[0], machine_id, "uv_exit", "wait_failed", None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uv_unknown_invalid_unicode_stderr_is_other_and_remains_secret() {
        let (runner, base) = setup(
            "#!/bin/sh\nprintf '\\377SENTINEL_CHILD_OUTPUT' >&2\nexit 24\n",
            2,
        );
        let machine = machine("SENTINEL_HOST", 22, Some(PIN));
        let machine_id = machine.id;
        let (result, events) = captured_worker(&runner, machine, runner.config.clone()).await;

        assert_eq!(result.unwrap_err(), DeployError::ExecutionOutcomeUnknown);
        assert_eq!(events.len(), 1);
        assert_failure_event(&events[0], machine_id, "uv_exit", "other", Some(24));
        assert_event_excludes(&events[0], &["SENTINEL_HOST", "SENTINEL_CHILD_OUTPUT"]);
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uv_signal_exit_emits_other_without_exit_code() {
        let (runner, base) = setup(
            "#!/bin/sh\nprintf SENTINEL_CHILD_OUTPUT >&2\nkill -TERM $$\n",
            2,
        );
        let machine = machine("SENTINEL_HOST", 22, Some(PIN));
        let machine_id = machine.id;
        let (result, events) = captured_worker(&runner, machine, runner.config.clone()).await;

        assert_eq!(result.unwrap_err(), DeployError::ExecutionOutcomeUnknown);
        assert_eq!(events.len(), 1);
        assert_failure_event(&events[0], machine_id, "uv_exit", "other", None);
        assert_event_excludes(&events[0], &["SENTINEL_HOST", "SENTINEL_CHILD_OUTPUT"]);
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn successful_uv_emits_no_diagnostic_event() {
        let script = format!(
            "#!/bin/sh\nprintf SENTINEL_SUCCESS_STDERR >&2\nprintf '%b' {output:?}\n",
            output = output(),
        );
        let (runner, base) = setup(&script, 2);
        let machine = machine("SENTINEL_HOST", 22, Some(PIN));
        let (result, events) = captured_worker(&runner, machine, runner.config.clone()).await;

        assert!(result.is_ok());
        assert!(events.is_empty());
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uv_cancellation_emits_no_exit_event() {
        let (runner, base) = setup("#!/bin/sh\nsleep 30\n", 5);
        let (cancel, cancelled) = oneshot::channel();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let _ = cancel.send(());
        });
        let (result, events) = captured_worker_with_cancellation(
            &runner,
            machine("SENTINEL_HOST", 22, Some(PIN)),
            runner.config.clone(),
            cancelled,
        )
        .await;

        assert_eq!(result.unwrap_err(), DeployError::CancelledOutcomeUnknown);
        assert!(events.is_empty());
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uv_timeout_emits_no_exit_event() {
        let (runner, base) = setup("#!/bin/sh\nsleep 30\n", 1);
        let (result, events) = captured_worker(
            &runner,
            machine("SENTINEL_HOST", 22, Some(PIN)),
            runner.config.clone(),
        )
        .await;

        assert_eq!(result.unwrap_err(), DeployError::TimeoutOutcomeUnknown);
        assert!(events.is_empty());
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uv_stderr_overflow_emits_no_exit_event() {
        let (runner, base) = setup(
            "#!/bin/sh\nwhile :; do printf SENTINEL_CHILD_OUTPUT >&2; done\n",
            5,
        );
        let (result, events) = captured_worker(
            &runner,
            machine("SENTINEL_HOST", 22, Some(PIN)),
            runner.config.clone(),
        )
        .await;

        assert_eq!(
            result.unwrap_err(),
            DeployError::OutputTooLargeOutcomeUnknown
        );
        assert!(events.is_empty());
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn timeout_kills_process_group_and_cleans_files() {
        let pid_file = temporary().join("pid");
        let script = format!(
            "#!/bin/sh\nsleep 30 & child=$!\nprintf $child > {}\nwait\n",
            pid_file.display()
        );
        let (runner, base) = setup(&script, 1);
        assert_eq!(
            runner
                .run("system-info", machine("host", 22, Some(PIN)))
                .await
                .unwrap_err(),
            DeployError::TimeoutOutcomeUnknown
        );
        let pid = fs::read_to_string(&pid_file).unwrap();
        assert_reaped(&pid).await;
        assert_eq!(fs::read_dir(base.join("temp")).unwrap().count(), 0);
        fs::remove_dir_all(base).unwrap();
        fs::remove_dir_all(pid_file.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn cancellation_kills_descendants_and_overflow_is_bounded() {
        let pid_file = temporary().join("pid");
        let script = format!(
            "#!/bin/sh\nsleep 30 & child=$!\nprintf $child > {}\nwait\n",
            pid_file.display()
        );
        let (runner, base) = setup(&script, 10);
        let (cancel, cancelled) = oneshot::channel();
        let request = tokio::spawn(async move {
            let mut future = Box::pin(async {
                let _ = cancelled.await;
            });
            runner
                .run_cancelled(
                    "system-info",
                    machine("host", 22, Some(PIN)),
                    future.as_mut(),
                )
                .await
        });
        let pid = wait_pid(&pid_file).await;
        cancel.send(()).unwrap();
        assert_eq!(
            request.await.unwrap().unwrap_err(),
            DeployError::CancelledOutcomeUnknown
        );
        assert_reaped(&pid).await;
        fs::remove_dir_all(base).unwrap();
        fs::remove_dir_all(pid_file.parent().unwrap()).unwrap();

        let (runner, base) = setup(
            "#!/bin/sh\nwhile :; do printf xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx; done\n",
            5,
        );
        assert_eq!(
            runner
                .run("system-info", machine("host", 22, Some(PIN)))
                .await
                .unwrap_err(),
            DeployError::OutputTooLargeOutcomeUnknown
        );
        assert_eq!(fs::read_dir(base.join("temp")).unwrap().count(), 0);
        fs::remove_dir_all(base).unwrap();
    }

    async fn wait_pid(path: &Path) -> String {
        for _ in 0..100 {
            if let Ok(pid) = fs::read_to_string(path) {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("pid was not recorded")
    }

    async fn assert_reaped(pid: &str) {
        let process = PathBuf::from(format!("/proc/{pid}"));
        for _ in 0..100 {
            if !process.exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("process {pid} was not killed")
    }

    #[derive(Debug)]
    struct CapturedEvent {
        level: tracing::Level,
        target: String,
        fields: BTreeMap<String, serde_json::Value>,
    }

    struct EventCapture(Arc<Mutex<Vec<CapturedEvent>>>);

    impl<S> Layer<S> for EventCapture
    where
        S: Subscriber,
    {
        fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
            let mut fields = CapturedFields::default();
            event.record(&mut fields);
            self.0.lock().unwrap().push(CapturedEvent {
                level: *event.metadata().level(),
                target: event.metadata().target().to_owned(),
                fields: fields.0,
            });
        }
    }

    #[derive(Default)]
    struct CapturedFields(BTreeMap<String, serde_json::Value>);

    impl Visit for CapturedFields {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
            self.0.insert(
                field.name().to_owned(),
                serde_json::Value::String(format!("{value:?}").trim_matches('"').to_owned()),
            );
        }

        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            self.0.insert(
                field.name().to_owned(),
                serde_json::Value::String(value.to_owned()),
            );
        }

        fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
            self.0.insert(field.name().to_owned(), value.into());
        }
    }
}
