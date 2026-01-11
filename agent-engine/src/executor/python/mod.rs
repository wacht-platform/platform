pub mod nsjail;
pub use nsjail::NsJailExecutor;

use common::error::AppError;
use async_trait::async_trait;

#[async_trait]
pub trait PythonExecutor: Send + Sync {
    /// Execute a Python script securely from a file
    /// 
    /// # Arguments
    /// * `execution_root` - The root directory of the execution (workspace/etc)
    /// * `script_path` - Relative path to the script inside execution_root
    /// * `args` - Command line arguments to pass to the script
    async fn execute_script(
        &self, 
        execution_root: &std::path::Path,
        script_path: &std::path::Path, 
        args: Vec<String>,
        timeout_secs: u64
    ) -> Result<ExecutionResult, AppError>;
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExecutionResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u128,
}
