use std::time::Duration;

use async_trait::async_trait;
use wacht_sandbox_client::{
    CreateTaskSandboxRequest, CreateThreadSandboxRequest, ExecSandboxRequest,
    SandboxHandle as NatsHandle, SandboxMountSpec, SandboxNatsClient, SandboxNatsClientError,
};

use super::{
    ExecRequest, ExecResult, SandboxError, SandboxHandle, SandboxResult, SandboxRuntime,
    TaskSandboxSpec, ThreadSandboxSpec,
};

const DEFAULT_EXEC_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const EXEC_REQUEST_BUFFER: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct NatsSandboxRuntime {
    client: SandboxNatsClient,
}

impl NatsSandboxRuntime {
    pub fn new(client: SandboxNatsClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl SandboxRuntime for NatsSandboxRuntime {
    async fn ensure_thread_sandbox(
        &self,
        spec: ThreadSandboxSpec,
    ) -> SandboxResult<Box<dyn SandboxHandle>> {
        let handle = self
            .client
            .thread(&CreateThreadSandboxRequest {
                deployment_id: spec.deployment_id.clone(),
                thread_id: spec.thread_id.clone(),
                project_id: spec.project_id.clone(),
                agent_id: spec.agent_id.clone(),
            })
            .await
            .map_err(|err| {
                tracing::warn!(
                    deployment_id = %spec.deployment_id,
                    thread_id = %spec.thread_id,
                    error = %err,
                    "sandbox: ensure_thread failed",
                );
                map_client_error(err)
            })?;
        Ok(Box::new(NatsSandboxHandleAdapter { inner: handle }))
    }

    async fn ensure_task_sandbox(
        &self,
        spec: TaskSandboxSpec,
    ) -> SandboxResult<Box<dyn SandboxHandle>> {
        let mounts: Vec<SandboxMountSpec> = spec
            .mounts
            .iter()
            .map(|m| SandboxMountSpec {
                mount_path: m.mount_path.clone(),
                s3_relative_key: m.s3_relative_key.clone(),
                mode: m.mode.as_str().to_string(),
            })
            .collect();
        let handle = self
            .client
            .task(&CreateTaskSandboxRequest {
                deployment_id: spec.deployment_id.clone(),
                project_id: spec.project_id.clone(),
                task_key: spec.task_key.clone(),
                mounts,
            })
            .await
            .map_err(|err| {
                tracing::warn!(
                    deployment_id = %spec.deployment_id,
                    task_key = %spec.task_key,
                    error = %err,
                    "sandbox: ensure_task failed",
                );
                map_client_error(err)
            })?;
        Ok(Box::new(NatsSandboxHandleAdapter { inner: handle }))
    }
}

struct NatsSandboxHandleAdapter {
    inner: NatsHandle,
}

#[async_trait]
impl SandboxHandle for NatsSandboxHandleAdapter {
    fn id(&self) -> &str {
        self.inner.sandbox_id()
    }

    async fn exec(&self, request: ExecRequest) -> SandboxResult<ExecResult> {
        let in_guest_timeout = request.timeout.unwrap_or(DEFAULT_EXEC_TIMEOUT);
        let nats_timeout = in_guest_timeout + EXEC_REQUEST_BUFFER;

        let response = self
            .inner
            .exec(
                ExecSandboxRequest {
                    sandbox_id: String::new(),
                    command: request.command,
                    cwd: request.cwd,
                    env: request.env,
                    exec_id: request.exec_id,
                    timeout_ms: Some(in_guest_timeout.as_millis() as u64),
                },
                nats_timeout,
            )
            .await
            .map_err(map_client_error)?;

        let (stdout, stderr) = self
            .inner
            .read_exec_output(&response)
            .await
            .map_err(map_client_error)?;

        Ok(ExecResult {
            exec_id: response.exec_id,
            exit_code: response.exit_code,
            timed_out: response.timed_out,
            cancelled: response.cancelled,
            stdout,
            stderr,
            stdout_truncated: response.stdout.truncated,
            stderr_truncated: response.stderr.truncated,
        })
    }

    async fn cancel(&self, exec_id: &str) -> SandboxResult<bool> {
        self.inner.cancel(exec_id).await.map_err(map_client_error)
    }

    async fn delete(&self) -> SandboxResult<()> {
        self.inner.delete().await.map_err(map_client_error)?;
        Ok(())
    }

    async fn read_file(&self, path: &str) -> SandboxResult<Vec<u8>> {
        self.inner.read_file(path).await.map_err(map_client_error)
    }

    async fn write_file(&self, path: &str, content: &[u8]) -> SandboxResult<()> {
        self.inner
            .write_file(path, content)
            .await
            .map_err(map_client_error)
    }
}

fn map_client_error(err: SandboxNatsClientError) -> SandboxError {
    use wacht_sandbox_client::SandboxErrorKind;
    match err {
        SandboxNatsClientError::Nats(msg) => SandboxError::Transient(msg),
        SandboxNatsClientError::Daemon { message, kind } => match kind {
            SandboxErrorKind::NotFound => SandboxError::NotFound(message),
            SandboxErrorKind::Timeout => SandboxError::Timeout(message),
            SandboxErrorKind::Cancelled => SandboxError::Cancelled,
            SandboxErrorKind::NotConfigured | SandboxErrorKind::InvalidRequest => {
                SandboxError::Config(message)
            }
            SandboxErrorKind::Internal => SandboxError::Other(message),
        },
        SandboxNatsClientError::Decode(msg) => SandboxError::Other(format!("decode: {msg}")),
        SandboxNatsClientError::Placement(err) => SandboxError::Transient(err.to_string()),
    }
}
