use common::ResultExt;
use std::sync::Arc;
use std::time::Duration;

use common::error::AppError;
use serde::{Deserialize, Serialize};

use crate::sandbox::{ExecRequest, SandboxHandle};

#[derive(Debug, Serialize, Deserialize)]
pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Default per-command timeout. Bumped from 30s to 10min so long-running
/// agent ops (ffmpeg re-encodes, large model downloads, video stitching)
/// don't get SIGKILL'd mid-stream. Individual calls can override via the
/// `timeout_seconds` field on `ExecuteCommandParams`.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Hard upper cap on per-call overrides. The sandbox itself enforces a
/// 20-minute ceiling, so 30 min on this side just bounds bad inputs.
const MAX_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Clone)]
pub struct ShellExecutor {
    sandbox_handle: Arc<dyn SandboxHandle>,
    timeout: Duration,
}

impl ShellExecutor {
    pub fn new(sandbox_handle: Arc<dyn SandboxHandle>) -> Self {
        Self {
            sandbox_handle,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub async fn execute(&self, command_line: &str) -> Result<ShellOutput, AppError> {
        self.execute_with_timeout(command_line, None).await
    }

    /// Execute with an optional per-call timeout override.
    /// `None` → use the executor's configured default (10 min stock).
    /// `Some(secs)` → clamp to [1, MAX_TIMEOUT] and use that for this call.
    pub async fn execute_with_timeout(
        &self,
        command_line: &str,
        timeout_override_secs: Option<u64>,
    ) -> Result<ShellOutput, AppError> {
        let effective_timeout = timeout_override_secs
            .map(|s| Duration::from_secs(s.clamp(1, MAX_TIMEOUT.as_secs())))
            .unwrap_or(self.timeout);
        let result = self
            .sandbox_handle
            .exec(ExecRequest {
                command: vec!["bash".into(), "-lc".into(), command_line.to_string()],
                cwd: Some("/workspace".into()),
                env: Default::default(),
                timeout: Some(effective_timeout),
                exec_id: None,
            })
            .await
            .map_err_internal("sandbox exec")?;

        Ok(ShellOutput {
            stdout: String::from_utf8_lossy(&result.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
            exit_code: result.exit_code,
        })
    }
}
