use std::ffi::OsString;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

use async_trait::async_trait;

const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const DEFAULT_OUTPUT_LIMIT: usize = 1024 * 1024;

#[derive(Clone)]
pub struct ProcessRequest {
    pub program: OsString,
    pub arguments: Vec<OsString>,
    pub timeout: Duration,
    pub output_limit: usize,
}

impl ProcessRequest {
    pub fn bounded(
        program: impl Into<OsString>,
        arguments: Vec<OsString>,
        timeout: Duration,
    ) -> Self {
        Self {
            program: program.into(),
            arguments,
            timeout,
            output_limit: DEFAULT_OUTPUT_LIMIT,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ProcessOutput {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("process I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("process timed out")]
    TimedOut,
    #[error("process was cancelled")]
    Cancelled,
    #[error("process stream task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

#[async_trait]
pub trait ProcessExecutor: Send + Sync {
    async fn run(
        &self,
        request: ProcessRequest,
        cancellation: CancellationToken,
    ) -> Result<ProcessOutput, ProcessError>;
}

#[derive(Debug, Clone, Copy)]
pub struct SystemProcessExecutor;

#[async_trait]
impl ProcessExecutor for SystemProcessExecutor {
    async fn run(
        &self,
        request: ProcessRequest,
        cancellation: CancellationToken,
    ) -> Result<ProcessOutput, ProcessError> {
        run_process(request, cancellation).await
    }
}

pub async fn run_process(
    request: ProcessRequest,
    cancellation: CancellationToken,
) -> Result<ProcessOutput, ProcessError> {
    let mut command = Command::new(&request.program);
    command
        .args(&request.arguments)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);

    let mut child = command.spawn()?;
    let process_id = child.id();
    let stdout = child.stdout.take().expect("piped stdout must be present");
    let stderr = child.stderr.take().expect("piped stderr must be present");
    let stdout_task = tokio::spawn(read_bounded(stdout, request.output_limit));
    let stderr_task = tokio::spawn(read_bounded(stderr, request.output_limit));

    let outcome = tokio::select! {
        result = child.wait() => result.map_err(ProcessError::Io),
        () = cancellation.cancelled() => {
            terminate_tree(&mut child, process_id).await;
            Err(ProcessError::Cancelled)
        }
        () = tokio::time::sleep(request.timeout) => {
            terminate_tree(&mut child, process_id).await;
            Err(ProcessError::TimedOut)
        }
    };

    if let Err(error) = outcome {
        stdout_task.abort();
        stderr_task.abort();
        return Err(error);
    }
    let status = outcome.expect("error path returned above");
    let (stdout, stdout_truncated) = stdout_task.await??;
    let (stderr, stderr_truncated) = stderr_task.await??;
    Ok(ProcessOutput {
        exit_code: status.code(),
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

async fn read_bounded(
    mut reader: impl AsyncRead + Unpin,
    limit: usize,
) -> Result<(Vec<u8>, bool), std::io::Error> {
    let mut collected = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(collected.len());
        let retained = count.min(remaining);
        collected.extend_from_slice(&buffer[..retained]);
        truncated |= retained < count;
    }
    Ok((collected, truncated))
}

async fn terminate_tree(child: &mut Child, process_id: Option<u32>) {
    #[cfg(windows)]
    if let Some(process_id) = process_id {
        let _ = Command::new("taskkill.exe")
            .args(["/PID", &process_id.to_string(), "/T", "/F"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await;
    }
    let _ = process_id;
    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn powershell(script: &str, timeout: Duration) -> ProcessRequest {
        ProcessRequest::bounded(
            "powershell.exe",
            vec!["-NoProfile".into(), "-Command".into(), script.into()],
            timeout,
        )
    }

    #[tokio::test]
    async fn captures_bounded_output() {
        let mut request = powershell("[Console]::Out.Write('abcdefgh')", Duration::from_secs(5));
        request.output_limit = 4;
        let output = run_process(request, CancellationToken::new())
            .await
            .unwrap();

        assert_eq!(output.stdout, b"abcd");
        assert!(output.stdout_truncated);
    }

    #[tokio::test]
    async fn timeout_terminates_process() {
        let error = run_process(
            powershell("Start-Sleep -Seconds 10", Duration::from_millis(100)),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, ProcessError::TimedOut));
    }

    #[tokio::test]
    async fn cancellation_terminates_process() {
        let cancellation = CancellationToken::new();
        let cancel = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            cancel.cancel();
        });
        let error = run_process(
            powershell("Start-Sleep -Seconds 10", Duration::from_secs(20)),
            cancellation,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, ProcessError::Cancelled));
    }
}
