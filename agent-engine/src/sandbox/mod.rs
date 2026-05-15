pub mod factory;
pub mod nats_runtime;
pub mod self_healing;

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;

pub use factory::{init_shared_sandbox_runtime, shared_sandbox_runtime, SandboxRuntimeFactory};
pub use nats_runtime::NatsSandboxRuntime;

pub type SandboxResult<T> = Result<T, SandboxError>;

#[async_trait]
pub trait SandboxRuntime: Send + Sync {
    async fn ensure_thread_sandbox(
        &self,
        spec: ThreadSandboxSpec,
    ) -> SandboxResult<Box<dyn SandboxHandle>>;

    async fn ensure_task_sandbox(
        &self,
        spec: TaskSandboxSpec,
    ) -> SandboxResult<Box<dyn SandboxHandle>>;
}

#[async_trait]
pub trait SandboxHandle: Send + Sync {
    fn id(&self) -> &str;

    async fn exec(&self, request: ExecRequest) -> SandboxResult<ExecResult>;

    async fn cancel(&self, exec_id: &str) -> SandboxResult<bool>;

    async fn delete(&self) -> SandboxResult<()>;

    async fn reconcile_skills(
        &self,
        agent_id: &str,
        slugs: Vec<String>,
    ) -> SandboxResult<Vec<String>>;

    async fn read_file(&self, path: &str) -> SandboxResult<Vec<u8>> {
        use base64::Engine;
        let result = self
            .exec(ExecRequest {
                command: vec![
                    "bash".into(),
                    "-c".into(),
                    "base64 \"$1\" | tr -d '\\n'".into(),
                    "_".into(),
                    path.to_string(),
                ],
                cwd: None,
                env: BTreeMap::new(),
                timeout: Some(Duration::from_secs(60)),
                exec_id: None,
            })
            .await?;
        if result.exit_code != 0 {
            let detail = String::from_utf8_lossy(&result.stderr).trim().to_string();
            return Err(SandboxError::NotFound(format!("read {path}: {detail}")));
        }
        let trimmed: Vec<u8> = result
            .stdout
            .into_iter()
            .filter(|b| !b.is_ascii_whitespace())
            .collect();
        base64::engine::general_purpose::STANDARD
            .decode(&trimmed)
            .map_err(|e| SandboxError::Other(format!("base64 decode of {path}: {e}")))
    }

    async fn write_file(&self, path: &str, content: &[u8]) -> SandboxResult<()> {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(content);
        let result = self
            .exec(ExecRequest {
                command: vec![
                    "bash".into(),
                    "-c".into(),
                    "mkdir -p \"$(dirname \"$2\")\" && printf '%s' \"$1\" | base64 -d > \"$2\""
                        .into(),
                    "_".into(),
                    b64,
                    path.to_string(),
                ],
                cwd: None,
                env: BTreeMap::new(),
                timeout: Some(Duration::from_secs(60)),
                exec_id: None,
            })
            .await?;
        if result.exit_code != 0 {
            let detail = String::from_utf8_lossy(&result.stderr).trim().to_string();
            return Err(SandboxError::Other(format!("write {path}: {detail}")));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ThreadSandboxSpec {
    pub deployment_id: String,
    pub thread_id: String,
    pub project_id: Option<String>,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskSandboxSpec {
    pub deployment_id: String,
    pub project_id: String,
    pub task_key: String,
    pub mounts: Vec<SandboxMount>,
}

#[derive(Debug, Clone)]
pub struct SandboxMount {
    pub mount_path: String,
    pub s3_relative_key: String,
    pub mode: SandboxMountMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxMountMode {
    Rw,
    Ro,
}

impl SandboxMountMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rw => "rw",
            Self::Ro => "ro",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecRequest {
    pub command: Vec<String>,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub timeout: Option<Duration>,
    pub exec_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExecResult {
    pub exec_id: String,
    pub exit_code: i32,
    pub timed_out: bool,
    pub cancelled: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("sandbox not found: {0}")]
    NotFound(String),
    #[error("timed out: {0}")]
    Timeout(String),
    #[error("cancelled")]
    Cancelled,
    #[error("transient: {0}")]
    Transient(String),
    #[error("config: {0}")]
    Config(String),
    #[error("other: {0}")]
    Other(String),
}
