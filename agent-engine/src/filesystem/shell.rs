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

#[derive(Clone)]
pub struct ShellExecutor {
    sandbox_handle: Arc<dyn SandboxHandle>,
    timeout: Duration,
}

impl ShellExecutor {
    pub fn new(sandbox_handle: Arc<dyn SandboxHandle>) -> Self {
        Self {
            sandbox_handle,
            timeout: Duration::from_secs(30),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub async fn execute(&self, command_line: &str) -> Result<ShellOutput, AppError> {
        let result = self
            .sandbox_handle
            .exec(ExecRequest {
                command: vec!["bash".into(), "-lc".into(), command_line.to_string()],
                cwd: Some("/workspace".into()),
                env: Default::default(),
                timeout: Some(self.timeout),
                exec_id: None,
            })
            .await
            .map_err(|err| AppError::Internal(format!("sandbox exec: {err}")))?;

        Ok(ShellOutput {
            stdout: String::from_utf8_lossy(&result.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
            exit_code: result.exit_code,
        })
    }
}
