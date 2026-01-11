use common::error::AppError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

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
                "cat",
                "head",
                "tail",
                "grep",
                "rg",
                "find",
                "ls",
                "tree",
                "wc",
                "du",
                "df",
                "touch",
                "mkdir",
                "echo",
                "cp",
                "mv",
                "rm",
                "chmod",
                "sed",
                "awk",
                "sort",
                "uniq",
                "jq",
                "cut",
                "tr",
                "diff",
                "date",
                "whoami",
                "pwd",
                "printf",
                "pdftotext",
                "file",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        }
    }

    pub async fn execute(&self, command_line: &str) -> Result<ShellOutput, AppError> {
        let command_line = self.normalize_path_aliases(command_line);

        let cmd_parts: Vec<&str> = command_line.split_whitespace().collect();
        let cmd_name = cmd_parts
            .first()
            .ok_or(AppError::BadRequest("Empty command".to_string()))?;

        if !self.allowed_commands.contains(&cmd_name.to_string()) {
            return Err(AppError::Forbidden(format!(
                "Command '{}' is not allowed",
                cmd_name
            )));
        }

        if command_line.contains("..") {
            return Err(AppError::Forbidden(
                "Path traversal (..) is not allowed in commands".to_string(),
            ));
        }

        let working_dir_str = self.working_dir.to_string_lossy().to_string();
        for part in command_line.split_whitespace() {
            if part.starts_with('/') {
                // Allow "//" which is a common operator in jq (null coalescing) and other tools
                if part == "//" {
                    continue;
                }

                if !part.starts_with(&working_dir_str) {
                    return Err(AppError::Forbidden(format!(
                        "Absolute path '{}' is outside allowed directories. Use /teams-activity/, /knowledge/, /uploads/, /scratch/, or /workspace/",
                        part
                    )));
                }
            }
        }

        let result = timeout(
            Duration::from_secs(self.timeout_secs),
            Command::new("bash")
                .arg("-c")
                .arg(&command_line)
                .current_dir(&self.working_dir)
                .output(),
        )
        .await
        .map_err(|_| AppError::Timeout)?;

        match result {
            Ok(output) => Ok(ShellOutput {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: output.status.code().unwrap_or(-1),
            }),
            Err(e) => Err(AppError::Internal(format!(
                "Failed to execute process: {}",
                e
            ))),
        }
    }

    fn normalize_path_aliases(&self, command_line: &str) -> String {
        let working_dir_str = self.working_dir.to_string_lossy().to_string();
        let base = working_dir_str.trim_end_matches('/');

        let words: Vec<&str> = command_line.split_whitespace().collect();
        let processed_words: Vec<String> = words
            .iter()
            .map(|word| {
                let aliases: Vec<(&str, String)> = vec![
                    ("/teams-activity/", format!("{}/teams-activity/", base)),
                    ("/teams-activity", format!("{}/teams-activity", base)),
                    ("/knowledge/", format!("{}/knowledge/", base)),
                    ("/knowledge", format!("{}/knowledge", base)),
                    ("/uploads/", format!("{}/uploads/", base)),
                    ("/uploads", format!("{}/uploads", base)),
                    ("/scratch/", format!("{}/scratch/", base)),
                    ("/scratch", format!("{}/scratch", base)),
                    ("/workspace/", format!("{}/workspace/", base)),
                    ("/workspace", format!("{}/workspace", base)),
                ];

                for (alias, replacement) in aliases {
                    if word.starts_with(alias) {
                        return format!("{}{}", replacement, &word[alias.len()..]);
                    }
                }
                word.to_string()
            })
            .collect();

        processed_words.join(" ")
    }

    pub async fn apply_pipeline(
        &self,
        input: &str,
        pipeline: &[String],
    ) -> Result<String, AppError> {
        if pipeline.is_empty() {
            return Ok(input.to_string());
        }

        let pipeline: Vec<String> = pipeline
            .iter()
            .map(|cmd| self.normalize_path_aliases(cmd))
            .collect();

        // Read-only commands allowed in pipeline (no rm, mv, cp, etc.)
        let pipeline_allowed: Vec<&str> = vec![
            "cat", "head", "tail", "grep", "rg", "wc", "sort", "uniq", "jq", "cut", "tr", "awk",
            "sed", "diff", "tee",
        ];

        // Validate each pipeline command
        let working_dir_str = self.working_dir.to_string_lossy().to_string();
        for cmd in &pipeline {
            let cmd_name = cmd.split_whitespace().next().unwrap_or("");
            if !pipeline_allowed.contains(&cmd_name) {
                return Err(AppError::Forbidden(format!(
                    "Command '{}' is not allowed in pipeline. Allowed: {:?}",
                    cmd_name, pipeline_allowed
                )));
            }
            if cmd.contains("..") {
                return Err(AppError::Forbidden(
                    "Path traversal (..) is not allowed in pipeline".to_string(),
                ));
            }
            // Block absolute paths outside working directory in pipeline
            for part in cmd.split_whitespace() {
                if part.starts_with('/') && !part.starts_with(&working_dir_str) {
                    // Allow "//" which is a common operator in jq (null coalescing)
                    if part == "//" {
                        continue;
                    }

                    return Err(AppError::Forbidden(format!(
                        "Absolute path '{}' is outside allowed directories in pipeline. Use /teams-activity/, /knowledge/, /uploads/, /scratch/, or /workspace/",
                        part
                    )));
                }
            }
        }

        // Build full pipeline command: echo "input" | cmd1 | cmd2 | cmd3
        let pipeline_str = pipeline.join(" | ");
        let full_command = format!("echo {} | {}", shell_escape(input), pipeline_str);

        let result = timeout(
            Duration::from_secs(10), // Shorter timeout for pipelines
            Command::new("bash")
                .arg("-c")
                .arg(&full_command)
                .current_dir(&self.working_dir)
                .output(),
        )
        .await
        .map_err(|_| AppError::Timeout)?;

        match result {
            Ok(output) => {
                if output.status.success() {
                    Ok(String::from_utf8_lossy(&output.stdout).to_string())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(AppError::Internal(format!("Pipeline failed: {}", stderr)))
                }
            }
            Err(e) => Err(AppError::Internal(format!(
                "Failed to execute pipeline: {}",
                e
            ))),
        }
    }
}

fn shell_escape(s: &str) -> String {
    // Use $'...' syntax for proper escaping
    format!(
        "$'{}'",
        s.replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\n', "\\n")
    )
}
