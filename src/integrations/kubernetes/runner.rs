use std::{future::Future, path::PathBuf, pin::Pin, process::Stdio, sync::Arc, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    sync::{Semaphore, oneshot, oneshot::error::TryRecvError},
    task::JoinHandle,
    time::Instant,
};

use super::Error;

const MAX_STDOUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 32 * 1024;

#[derive(Clone)]
pub(super) struct Runner {
    executable: Arc<PathBuf>,
    kubeconfig: Arc<PathBuf>,
    cache_dir: Arc<PathBuf>,
    context: Arc<String>,
    request_timeout: Arc<String>,
    permits: Arc<Semaphore>,
    deadline: Duration,
    #[cfg(test)]
    force_stdout_read_failure: bool,
}

#[derive(Debug)]
pub(super) struct Output {
    pub stdout: Vec<u8>,
}

#[derive(Clone, Copy)]
pub(super) enum Operation {
    Query,
    Mutation,
}

impl Runner {
    pub fn new(
        executable: PathBuf,
        kubeconfig: PathBuf,
        cache_dir: PathBuf,
        context: String,
        deadline: Duration,
    ) -> Result<Self, String> {
        if !executable.is_absolute()
            || !kubeconfig.is_absolute()
            || !cache_dir.is_absolute()
            || !valid_context(&context)
            || deadline < Duration::from_millis(2)
            || deadline > Duration::from_secs(30)
        {
            return Err("invalid Kubernetes runner configuration".into());
        }
        let outer_millis = deadline.as_millis();
        let margin = (outer_millis / 6).clamp(1, 5_000);
        let request_timeout = format!("{}ms", outer_millis - margin);
        Ok(Self {
            executable: Arc::new(executable),
            kubeconfig: Arc::new(kubeconfig),
            cache_dir: Arc::new(cache_dir),
            context: Arc::new(context),
            request_timeout: Arc::new(request_timeout),
            permits: Arc::new(Semaphore::new(2)),
            deadline,
            #[cfg(test)]
            force_stdout_read_failure: false,
        })
    }

    #[cfg(test)]
    pub async fn run(&self, arguments: Vec<String>, operation: Operation) -> Result<Output, Error> {
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::CapacityExhausted)?;
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let (result_tx, result_rx) = oneshot::channel();
        let runner = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let result = worker(&runner, arguments, operation, cancel_rx).await;
            let _ = result_tx.send(result);
        });
        let result = result_rx
            .await
            .map_err(|_| operation_failure(operation, true))?;
        drop(cancel_tx);
        result
    }

    pub async fn run_cancelled(
        &self,
        arguments: Vec<String>,
        operation: Operation,
        mut cancellation: Pin<&mut (dyn Future<Output = ()> + Send)>,
    ) -> Result<Output, Error> {
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::CapacityExhausted)?;
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let (result_tx, mut result_rx) = oneshot::channel();
        let runner = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let result = worker(&runner, arguments, operation, cancel_rx).await;
            let _ = result_tx.send(result);
        });
        tokio::select! {
            biased;
            result = &mut result_rx => {
                drop(cancel_tx);
                result.map_err(|_| operation_failure(operation, true))?
            }
            () = cancellation.as_mut() => {
                let _ = cancel_tx.send(());
                // Do not report cancellation until the worker has killed and reaped the child.
                let _ = result_rx.await;
                Err(match operation {
                    Operation::Query => Error::RequestCancelled,
                    Operation::Mutation => Error::MutationOutcomeUnknown,
                })
            }
        }
    }
}

async fn worker(
    runner: &Runner,
    arguments: Vec<String>,
    operation: Operation,
    mut cancelled: oneshot::Receiver<()>,
) -> Result<Output, Error> {
    match cancelled.try_recv() {
        Ok(()) | Err(TryRecvError::Closed) => return Err(operation_failure(operation, false)),
        Err(TryRecvError::Empty) => {}
    }
    let mut command = Command::new(runner.executable.as_path());
    command
        .env_clear()
        .env("KUBECTL_KUBERC", "false")
        .arg("--kubeconfig")
        .arg(runner.kubeconfig.as_path())
        .arg("--cache-dir")
        .arg(runner.cache_dir.as_path())
        .arg("--context")
        .arg(runner.context.as_str())
        .arg("--request-timeout")
        .arg(runner.request_timeout.as_str())
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|_| operation_failure(operation, false))?;
    let Some(stdout) = child.stdout.take() else {
        terminate(&mut child, None, None).await;
        return Err(operation_failure(operation, true));
    };
    let Some(stderr) = child.stderr.take() else {
        terminate(&mut child, None, None).await;
        return Err(operation_failure(operation, true));
    };
    #[cfg(test)]
    let stdout_task = if runner.force_stdout_read_failure {
        tokio::spawn(async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            Err(())
        })
    } else {
        tokio::spawn(drain(stdout, MAX_STDOUT_BYTES))
    };
    #[cfg(not(test))]
    let stdout_task = tokio::spawn(drain(stdout, MAX_STDOUT_BYTES));
    let mut stdout_task = Some(stdout_task);
    let mut stderr_task = Some(tokio::spawn(drain(stderr, MAX_STDERR_BYTES)));
    let expires = Instant::now() + runner.deadline;
    let mut status: Option<std::process::ExitStatus> = None;
    let mut stdout_bytes: Option<Vec<u8>> = None;
    let mut stderr_bytes: Option<Vec<u8>> = None;

    loop {
        if status.is_some() && stdout_bytes.is_some() && stderr_bytes.is_some() {
            let status = status.take().expect("checked");
            let stdout = stdout_bytes.take().expect("checked");
            let stderr = stderr_bytes.take().expect("checked");
            return if status.success() {
                Ok(Output { stdout })
            } else {
                Err(if matches!(operation, Operation::Mutation) {
                    Error::MutationOutcomeUnknown
                } else {
                    classify_exit(status.code(), &stderr)
                })
            };
        }
        tokio::select! {
            biased;
            _ = &mut cancelled => {
                terminate(&mut child, stdout_task.take(), stderr_task.take()).await;
                return Err(operation_failure(operation, true));
            }
            _ = tokio::time::sleep_until(expires) => {
                terminate(&mut child, stdout_task.take(), stderr_task.take()).await;
                return Err(if matches!(operation, Operation::Mutation) { Error::MutationOutcomeUnknown } else { Error::Timeout });
            }
            result = child.wait(), if status.is_none() => {
                match result {
                    Ok(exit) => status = Some(exit),
                    Err(_) => {
                        terminate(&mut child, stdout_task.take(), stderr_task.take()).await;
                        return Err(operation_failure(operation, true));
                    }
                }
            }
            result = async { stdout_task.as_mut().expect("guarded").await }, if stdout_task.is_some() => {
                stdout_task = None;
                match result {
                    Ok(Ok(bytes)) => stdout_bytes = Some(bytes),
                    _ => {
                        terminate(&mut child, None, stderr_task.take()).await;
                        return Err(stream_failure(operation));
                    }
                }
            }
            result = async { stderr_task.as_mut().expect("guarded").await }, if stderr_task.is_some() => {
                stderr_task = None;
                match result {
                    Ok(Ok(bytes)) => stderr_bytes = Some(bytes),
                    _ => {
                        terminate(&mut child, stdout_task.take(), None).await;
                        return Err(stream_failure(operation));
                    }
                }
            }
        }
    }
}

async fn drain(mut stream: impl AsyncRead + Unpin, limit: usize) -> Result<Vec<u8>, ()> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
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

async fn terminate(
    child: &mut tokio::process::Child,
    stdout: Option<JoinHandle<Result<Vec<u8>, ()>>>,
    stderr: Option<JoinHandle<Result<Vec<u8>, ()>>>,
) {
    let _ = child.start_kill();
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

fn classify_exit(code: Option<i32>, stderr: &[u8]) -> Error {
    let message = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    if message.contains("forbidden") || message.contains("unauthorized") {
        Error::Unauthorized
    } else if message.contains("the server could not find the requested resource")
        || message.contains("doesn't have a resource type")
    {
        Error::UnsupportedApi
    } else if message.contains("notfound") || message.contains("not found") {
        Error::NotFound
    } else {
        match code {
            Some(1) => Error::QueryRejected,
            Some(2) => Error::InvalidArguments,
            _ => Error::UpstreamUnavailable,
        }
    }
}

fn operation_failure(operation: Operation, dispatched: bool) -> Error {
    match (operation, dispatched) {
        (Operation::Mutation, true) => Error::MutationOutcomeUnknown,
        (Operation::Mutation, false) => Error::MutationRejected,
        (Operation::Query, _) => Error::UpstreamUnavailable,
    }
}

fn stream_failure(operation: Operation) -> Error {
    if matches!(operation, Operation::Mutation) {
        Error::MutationOutcomeUnknown
    } else {
        Error::ResponseTooLarge
    }
}

fn valid_context(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && !value.starts_with('-')
        && !value.chars().any(|c| c.is_control() || c.is_whitespace())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | ':' | '/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs, io,
        os::unix::fs::PermissionsExt,
        pin::Pin,
        sync::atomic::{AtomicU64, Ordering},
        task::{Context, Poll},
    };
    use tokio::io::ReadBuf;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct BrokenReader;

    impl AsyncRead for BrokenReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            _buffer: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Ready(Err(io::Error::other("test read failure")))
        }
    }

    fn pid_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "homelab-kube-pid-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    async fn wait_for_pid(path: &PathBuf) -> String {
        for _ in 0..100 {
            if let Ok(pid) = fs::read_to_string(path) {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("process pid was not recorded")
    }

    async fn assert_reaped(pid: &str) {
        let process = PathBuf::from(format!("/proc/{pid}"));
        for _ in 0..100 {
            if !process.exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("process {pid} was not reaped")
    }
    fn script(body: &str, deadline: Duration) -> (Runner, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "homelab-kube-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).unwrap();
        (
            Runner::new(
                path.clone(),
                PathBuf::from("/tmp/fixed-kubeconfig"),
                PathBuf::from("/tmp/fixed-kubectl-cache"),
                "fixed-context".into(),
                deadline,
            )
            .unwrap(),
            path,
        )
    }

    #[tokio::test]
    async fn inherited_secrets_and_proxies_are_cleared() {
        let (runner, path) = script("env", Duration::from_secs(2));
        let output = runner.run(vec![], Operation::Query).await.unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        for inherited in ["PATH=", "HOME=", "HTTPS_PROXY=", "KUBECONFIG="] {
            assert!(!text.lines().any(|line| line.starts_with(inherited)));
        }
        assert!(text.lines().any(|line| line == "KUBECTL_KUBERC=false"));
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn dual_streams_are_bounded_without_deadlock() {
        let (runner, path) = script(
            "i=0; while [ $i -lt 600 ]; do printf 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'; i=$((i+1)); done; i=0; while [ $i -lt 100 ]; do printf 'yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy' >&2; i=$((i+1)); done",
            Duration::from_secs(5),
        );
        let output = runner.run(vec![], Operation::Query).await.unwrap();
        assert!(output.stdout.len() > MAX_STDERR_BYTES && output.stdout.len() < MAX_STDOUT_BYTES);
        fs::remove_file(path).unwrap();

        let (runner, path) = script("kill -KILL $$", Duration::from_secs(2));
        assert_eq!(
            runner.run(vec![], Operation::Mutation).await.unwrap_err(),
            Error::MutationOutcomeUnknown
        );
        fs::remove_file(path).unwrap();

        assert_eq!(
            drain(&vec![b'x'; MAX_STDOUT_BYTES][..], MAX_STDOUT_BYTES)
                .await
                .unwrap()
                .len(),
            MAX_STDOUT_BYTES
        );
        assert!(
            drain(&vec![b'x'; MAX_STDOUT_BYTES + 1][..], MAX_STDOUT_BYTES)
                .await
                .is_err()
        );
        assert_eq!(
            drain(&vec![b'y'; MAX_STDERR_BYTES][..], MAX_STDERR_BYTES)
                .await
                .unwrap()
                .len(),
            MAX_STDERR_BYTES
        );
        assert!(
            drain(&vec![b'y'; MAX_STDERR_BYTES + 1][..], MAX_STDERR_BYTES)
                .await
                .is_err()
        );

        let stderr_pid_path = pid_path();
        let (runner, path) = script(
            &format!(
                "printf $$ > {}; while :; do printf 'yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy' >&2; done",
                stderr_pid_path.display()
            ),
            Duration::from_secs(5),
        );
        assert_eq!(
            runner.run(vec![], Operation::Query).await.unwrap_err(),
            Error::ResponseTooLarge
        );
        let pid = wait_for_pid(&stderr_pid_path).await;
        assert_reaped(&pid).await;
        assert_eq!(runner.permits.available_permits(), 2);
        fs::remove_file(stderr_pid_path).unwrap();
        fs::remove_file(path).unwrap();

        let stdout_pid_path = pid_path();
        let (runner, path) = script(
            &format!(
                "printf $$ > {}; while :; do printf 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'; done",
                stdout_pid_path.display()
            ),
            Duration::from_secs(5),
        );
        assert_eq!(
            runner.run(vec![], Operation::Query).await.unwrap_err(),
            Error::ResponseTooLarge
        );
        let pid = wait_for_pid(&stdout_pid_path).await;
        assert_reaped(&pid).await;
        assert_eq!(runner.permits.available_permits(), 2);
        fs::remove_file(stdout_pid_path).unwrap();
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn timeout_kills_and_releases_capacity() {
        let pid_path = pid_path();
        let (runner, path) = script(
            &format!("printf $$ > {}; exec /usr/bin/sleep 10", pid_path.display()),
            Duration::from_millis(200),
        );
        assert_eq!(
            runner.run(vec![], Operation::Query).await.unwrap_err(),
            Error::Timeout
        );
        let pid = wait_for_pid(&pid_path).await;
        assert_reaped(&pid).await;
        assert_eq!(runner.permits.available_permits(), 2);
        assert_eq!(
            runner.run(vec![], Operation::Query).await.unwrap_err(),
            Error::Timeout
        );
        fs::remove_file(pid_path).unwrap();
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn read_failure_kills_reaps_and_releases_capacity() {
        let pid_path = pid_path();
        let (mut runner, path) = script(
            &format!("printf $$ > {}; exec /usr/bin/sleep 10", pid_path.display()),
            Duration::from_secs(2),
        );
        runner.force_stdout_read_failure = true;
        assert_eq!(
            runner.run(vec![], Operation::Query).await.unwrap_err(),
            Error::ResponseTooLarge
        );
        let pid = wait_for_pid(&pid_path).await;
        assert_reaped(&pid).await;
        assert_eq!(runner.permits.available_permits(), 2);
        fs::remove_file(pid_path).unwrap();
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn cancellation_kills_reaps_and_releases_capacity() {
        let pid_path = pid_path();
        let (runner, path) = script(
            &format!("printf $$ > {}; exec /usr/bin/sleep 10", pid_path.display()),
            Duration::from_secs(5),
        );
        let first = tokio::spawn({
            let runner = runner.clone();
            async move { runner.run(vec![], Operation::Query).await }
        });
        let pid = wait_for_pid(&pid_path).await;
        first.abort();
        let _ = first.await;
        assert_reaped(&pid).await;
        let second = tokio::spawn({
            let runner = runner.clone();
            async move { runner.run(vec![], Operation::Query).await }
        });
        let third = tokio::spawn({
            let runner = runner.clone();
            async move { runner.run(vec![], Operation::Query).await }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            runner.run(vec![], Operation::Query).await.unwrap_err(),
            Error::CapacityExhausted
        );
        second.abort();
        third.abort();
        let _ = second.await;
        let _ = third.await;
        fs::remove_file(pid_path).unwrap();
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn explicit_query_and_mutation_cancellation_return_after_reap() {
        for (operation, expected) in [
            (Operation::Query, Error::RequestCancelled),
            (Operation::Mutation, Error::MutationOutcomeUnknown),
        ] {
            let pid_path = pid_path();
            let (runner, path) = script(
                &format!("printf $$ > {}; exec /usr/bin/sleep 10", pid_path.display()),
                Duration::from_secs(5),
            );
            let (cancel, cancelled) = oneshot::channel();
            let request = tokio::spawn({
                let runner = runner.clone();
                async move {
                    let mut cancelled = Box::pin(async move {
                        let _ = cancelled.await;
                    });
                    runner
                        .run_cancelled(vec![], operation, cancelled.as_mut())
                        .await
                }
            });
            let pid = wait_for_pid(&pid_path).await;
            cancel.send(()).unwrap();
            assert_eq!(request.await.unwrap().unwrap_err(), expected);
            assert!(!PathBuf::from(format!("/proc/{pid}")).exists());
            assert_eq!(runner.permits.available_permits(), 2);
            fs::remove_file(pid_path).unwrap();
            fs::remove_file(path).unwrap();
        }
    }

    #[tokio::test]
    async fn errors_redact_stderr_and_mutation_ambiguity() {
        let (runner, path) = script("printf 'RAW-SECRET' >&2; exit 7", Duration::from_secs(2));
        let error = runner.run(vec![], Operation::Query).await.unwrap_err();
        assert_eq!(error, Error::UpstreamUnavailable);
        assert!(!format!("{error:?}").contains("RAW-SECRET"));
        fs::remove_file(path).unwrap();

        let (runner, path) = script("/usr/bin/sleep 10", Duration::from_millis(50));
        assert_eq!(
            runner.run(vec![], Operation::Mutation).await.unwrap_err(),
            Error::MutationOutcomeUnknown
        );
        fs::remove_file(path).unwrap();

        let (runner, path) = script(
            "printf 'rejected detail' >&2; exit 1",
            Duration::from_secs(2),
        );
        let error = runner.run(vec![], Operation::Mutation).await.unwrap_err();
        assert_eq!(error, Error::MutationOutcomeUnknown);
        assert!(!format!("{error:?}").contains("rejected detail"));
        fs::remove_file(path).unwrap();

        let runner = Runner::new(
            PathBuf::from("/definitely/missing/kubectl"),
            PathBuf::from("/tmp/kubeconfig"),
            PathBuf::from("/tmp/kubectl-cache"),
            "context".into(),
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(
            runner.run(vec![], Operation::Mutation).await.unwrap_err(),
            Error::MutationRejected
        );
    }

    #[test]
    fn request_timeout_is_nonzero_and_strictly_inside_every_outer_deadline() {
        for deadline in [
            Duration::from_millis(2),
            Duration::from_secs(5),
            Duration::from_millis(5_001),
            Duration::from_secs(6),
            Duration::from_secs(30),
        ] {
            let runner = Runner::new(
                PathBuf::from("/usr/bin/kubectl"),
                PathBuf::from("/tmp/kubeconfig"),
                PathBuf::from("/tmp/kubectl-cache"),
                "context".into(),
                deadline,
            )
            .unwrap();
            let millis = runner
                .request_timeout
                .strip_suffix("ms")
                .unwrap()
                .parse::<u128>()
                .unwrap();
            assert!(millis > 0 && millis < deadline.as_millis());
        }
        assert!(
            Runner::new(
                PathBuf::from("/usr/bin/kubectl"),
                PathBuf::from("/tmp/kubeconfig"),
                PathBuf::from("/tmp/kubectl-cache"),
                "context".into(),
                Duration::from_millis(1),
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn mutation_cancellation_and_stream_failures_are_uncertain_and_reaped() {
        let cancel_pid_path = pid_path();
        let (runner, path) = script(
            &format!(
                "printf $$ > {}; exec /usr/bin/sleep 10",
                cancel_pid_path.display()
            ),
            Duration::from_secs(5),
        );
        let request = tokio::spawn({
            let runner = runner.clone();
            async move { runner.run(vec![], Operation::Mutation).await }
        });
        let pid = wait_for_pid(&cancel_pid_path).await;
        request.abort();
        let _ = request.await;
        assert_reaped(&pid).await;
        fs::remove_file(cancel_pid_path).unwrap();
        fs::remove_file(path).unwrap();

        let overflow_pid_path = pid_path();
        let (runner, path) = script(
            &format!(
                "printf $$ > {}; while :; do printf 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'; done",
                overflow_pid_path.display()
            ),
            Duration::from_secs(5),
        );
        assert_eq!(
            runner.run(vec![], Operation::Mutation).await.unwrap_err(),
            Error::MutationOutcomeUnknown
        );
        let pid = wait_for_pid(&overflow_pid_path).await;
        assert_reaped(&pid).await;
        fs::remove_file(overflow_pid_path).unwrap();
        fs::remove_file(path).unwrap();

        assert!(drain(BrokenReader, MAX_STDOUT_BYTES).await.is_err());
        assert_eq!(
            stream_failure(Operation::Mutation),
            Error::MutationOutcomeUnknown
        );
    }
}
