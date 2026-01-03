use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use common::error::AppError;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct ShellExecutor {
    working_dir: PathBuf,
    timeout_secs: u64,
    allowed_commands: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl ShellExecutor {
    pub fn new(working_dir: PathBuf) -> Self {
        Self {
            working_dir,
            timeout_secs: 30,
            allowed_commands: vec![
                "cat", "head", "tail", "grep", "rg", "find", "ls", "tree", "wc", "du", "df",
                "touch", "mkdir", "echo", "cp", "mv", "rm", "chmod",
                "sed", "awk", "sort", "uniq", "jq", "cut", "tr", "diff",
                "date", "whoami", "pwd", "printf"
            ].iter().map(|s| s.to_string()).collect(),
        }
    }
    
    pub async fn execute(&self, command_line: &str) -> Result<ShellOutput, AppError> {
        let cmd_parts: Vec<&str> = command_line.split_whitespace().collect();
        let cmd_name = cmd_parts.first().ok_or(AppError::BadRequest("Empty command".to_string()))?;
        
        if !self.allowed_commands.contains(&cmd_name.to_string()) {
            return Err(AppError::Forbidden(format!("Command '{}' is not allowed", cmd_name)));
        }

        if command_line.contains("..") {
             return Err(AppError::Forbidden("Path traversal (..) is not allowed in commands".to_string()));
        }
        
        let result = timeout(
            Duration::from_secs(self.timeout_secs),
            Command::new("bash")
                .arg("-c")
                .arg(command_line)
                .current_dir(&self.working_dir)
                .output()
        ).await.map_err(|_| AppError::Timeout)?;
        
        match result {
            Ok(output) => Ok(ShellOutput {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: output.status.code().unwrap_or(-1),
            }),
            Err(e) => Err(AppError::Internal(format!("Failed to execute process: {}", e))),
        }
    }
}
