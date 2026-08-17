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
            let _ = result_tx.send(worker(deploy, machine, signer, config, cancel_rx).await);
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
) -> Result<DeployResult, DeployError> {
    cancelled_now(&mut cancelled)?;
    let pin = machine
        .pinned_host_public_key
        .as_deref()
        .ok_or(DeployError::MissingHostPin)?;
    validate_host_pin(pin)?;
    let run_dir = create_run_dir(&config.temp_root)?;
    let _cleanup = Cleanup(run_dir.clone());
    let key = run_dir.join("identity");
    let known_hosts = run_dir.join("known_hosts");
    run_keygen(&config.ssh_keygen_executable, &key, &mut cancelled).await?;
    let public_key = read_bounded(&key.with_extension("pub"), 1024)?;
    cancelled_now(&mut cancelled)?;
    let certificate = Zeroizing::new(tokio::select! {
        result = signer.sign(public_key.trim_end()) => result?,
        _ = &mut cancelled => return Err(DeployError::Cancelled),
    });
    write_private(
        &key.with_file_name("identity-cert.pub"),
        certificate.as_bytes(),
        0o600,
    )?;
    write_private(
        &known_hosts,
        format!(
            "{} {}\n",
            known_hosts_host(&machine.ssh_host, machine.ssh_port),
            pin
        )
        .as_bytes(),
        0o600,
    )?;
    let inventory = serde_json::to_string(&json!({"version":1,"host":{"address":machine.ssh_host,"user":machine.ssh_username,"port":machine.ssh_port,"ssh_key":key,"known_hosts":known_hosts}}))
        .map_err(|_| DeployError::ExecutionRejected)?;
    let output = run_uv(
        &config.uv_executable,
        &config.temp_root,
        &deploy,
        &inventory,
        &mut cancelled,
    )
    .await?;
    normalize::system_info(&output)
}

async fn run_keygen(
    executable: &Path,
    key: &Path,
    cancelled: &mut oneshot::Receiver<()>,
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
    let mut child = command
        .spawn()
        .map_err(|_| DeployError::ExecutionRejected)?;
    let result = tokio::select! {
        result = child.wait() => match result { Ok(status) if status.success() => Ok(()), _ => Err(DeployError::ExecutionRejected) },
        _ = &mut *cancelled => { terminate_group(&mut child, None, None).await; Err(DeployError::Cancelled) },
        _ = tokio::time::sleep(Duration::from_secs(10)) => { terminate_group(&mut child, None, None).await; Err(DeployError::ExecutionRejected) }
    };
    result?;
    for path in [key.to_path_buf(), key.with_extension("pub")] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|_| DeployError::ExecutionRejected)?;
    }
    Ok(())
}

async fn run_uv(
    executable: &Path,
    temp_root: &Path,
    deploy: &ResolvedDeploy,
    inventory: &str,
    cancelled: &mut oneshot::Receiver<()>,
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
    let mut child = command
        .spawn()
        .map_err(|_| DeployError::ExecutionRejected)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(DeployError::ExecutionOutcomeUnknown)?;
    let stderr = child
        .stderr
        .take()
        .ok_or(DeployError::ExecutionOutcomeUnknown)?;
    let mut stdout_task = Some(tokio::spawn(drain(stdout, MAX_STDOUT_BYTES)));
    let mut stderr_task = Some(tokio::spawn(drain(stderr, MAX_STDERR_BYTES)));
    let expires = Instant::now() + Duration::from_secs(deploy.timeout_seconds);
    let mut status: Option<std::process::ExitStatus> = None;
    let mut output: Option<Vec<u8>> = None;
    let mut stderr_done = false;
    loop {
        if let (Some(status), Some(output)) = (status.as_ref(), output.as_ref())
            && stderr_done
        {
            return if status.success() {
                Ok(output.clone())
            } else {
                Err(DeployError::ExecutionOutcomeUnknown)
            };
        }
        tokio::select! {
            biased;
            _ = &mut *cancelled => { terminate_group(&mut child, stdout_task.take(), stderr_task.take()).await; return Err(DeployError::CancelledOutcomeUnknown); }
            _ = tokio::time::sleep_until(expires) => { terminate_group(&mut child, stdout_task.take(), stderr_task.take()).await; return Err(DeployError::TimeoutOutcomeUnknown); }
            result = child.wait(), if status.is_none() => match result { Ok(value) => status = Some(value), Err(_) => { terminate_group(&mut child, stdout_task.take(), stderr_task.take()).await; return Err(DeployError::ExecutionOutcomeUnknown); } },
            result = async { stdout_task.as_mut().expect("guarded").await }, if stdout_task.is_some() => { stdout_task = None; match result { Ok(Ok(bytes)) => output = Some(bytes), _ => { terminate_group(&mut child, None, stderr_task.take()).await; return Err(DeployError::OutputTooLargeOutcomeUnknown); } } },
            result = async { stderr_task.as_mut().expect("guarded").await }, if stderr_task.is_some() => { stderr_task = None; match result { Ok(Ok(_)) => stderr_done = true, _ => { terminate_group(&mut child, stdout_task.take(), None).await; return Err(DeployError::OutputTooLargeOutcomeUnknown); } } },
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
    stderr: Option<JoinHandle<Result<Vec<u8>, ()>>>,
) {
    if let Some(pid) = child.id() {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    let _ = child.wait().await;
    for task in [stdout, stderr].into_iter().flatten() {
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

fn create_run_dir(root: &Path) -> Result<PathBuf, DeployError> {
    for _ in 0..8 {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|_| DeployError::ExecutionRejected)?;
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
            Err(_) => return Err(DeployError::ExecutionRejected),
        }
    }
    Err(DeployError::ExecutionRejected)
}

fn write_private(path: &Path, bytes: &[u8], mode: u32) -> Result<(), DeployError> {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .map_err(|_| DeployError::ExecutionRejected)?;
    file.write_all(bytes)
        .map_err(|_| DeployError::ExecutionRejected)
}

fn read_bounded(path: &Path, limit: u64) -> Result<String, DeployError> {
    let metadata = std::fs::metadata(path).map_err(|_| DeployError::ExecutionRejected)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(DeployError::CredentialInvalid);
    }
    std::fs::read_to_string(path).map_err(|_| DeployError::CredentialInvalid)
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
        fs,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicU64, Ordering},
    };
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
assert pathlib.Path(host["ssh_key"] + "-cert.pub").read_text() == "ssh-ed25519-cert-v01@openssh.com AAACERT"
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
}
