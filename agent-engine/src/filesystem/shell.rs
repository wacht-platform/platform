use crate::filesystem::sandbox::SandboxRunner;
use common::error::AppError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone)]
pub struct ShellExecutor {
    working_dir: PathBuf,
    sandbox: SandboxRunner,
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
            sandbox: SandboxRunner::new(working_dir.clone()),
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
                "python",
                "python3",
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

        // Validate ALL commands in pipes, not just the first one
        // Use quote-aware splitting to handle jq expressions like '.foo | .bar'
        let command_segments = split_command_segments(&command_line);

        for segment in &command_segments {
            let trimmed = segment.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Get the first word of each segment (the command name)
            let cmd_name = trimmed.split_whitespace().next();

            if let Some(cmd) = cmd_name {
                // Skip if it's a subcommand indicator like "then", "else", "do", etc.
                let shell_keywords = ["then", "else", "fi", "do", "done", "esac", "in"];
                if shell_keywords.contains(&cmd) {
                    continue;
                }

                if !self.allowed_commands.contains(&cmd.to_string()) {
                    return Err(AppError::Forbidden(format!(
                        "Command '{}' is not allowed. Allowed commands: cat, grep, jq, head, tail, python3, etc.",
                        cmd
                    )));
                }
            }
        }

        for part in command_line.split_whitespace() {
            if part.starts_with('/') {
                // Allow "//" which is a common operator in jq (null coalescing) and other tools
                if part == "//" {
                    continue;
                }

                if !is_allowed_absolute_path(part) {
                    return Err(AppError::Forbidden(format!(
                        "Absolute path '{}' is outside allowed directories. Use /knowledge/, /uploads/, /scratch/, /workspace/, or /app/",
                        part
                    )));
                }
            }
        }

        let output = self
            .sandbox
            .execute_shell(&command_line, self.timeout_secs)
            .await?;

        Ok(ShellOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.exit_code,
        })
    }

    fn normalize_path_aliases(&self, command_line: &str) -> String {
        if self.sandbox.uses_virtual_alias_paths() {
            return command_line.to_string();
        }

        let working_dir_str = self.working_dir.to_string_lossy().to_string();
        let base = working_dir_str.trim_end_matches('/');

        let words: Vec<&str> = command_line.split_whitespace().collect();
        let processed_words: Vec<String> = words
            .iter()
            .map(|word| {
                let aliases: Vec<(&str, String)> = vec![
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
                if part.starts_with('/') && !is_allowed_absolute_path(part) {
                    // Allow "//" which is a common operator in jq (null coalescing)
                    if part == "//" {
                        continue;
                    }

                    return Err(AppError::Forbidden(format!(
                        "Absolute path '{}' is outside allowed directories in pipeline. Use /knowledge/, /uploads/, /scratch/, /workspace/, or /app/",
                        part
                    )));
                }
            }
        }

        // Build full pipeline command: echo "input" | cmd1 | cmd2 | cmd3
        let pipeline_str = pipeline.join(" | ");
        let full_command = format!("echo {} | {}", shell_escape(input), pipeline_str);

        let output = self.sandbox.execute_shell(&full_command, 10).await?;
        if output.exit_code == 0 {
            Ok(output.stdout)
        } else {
            Err(AppError::Internal(format!(
                "Pipeline failed: {}",
                output.stderr
            )))
        }
    }
}

fn is_allowed_absolute_path(path: &str) -> bool {
    [
        "/knowledge",
        "/uploads",
        "/scratch",
        "/workspace",
        "/app",
        "/tmp",
    ]
    .iter()
    .any(|prefix| path == *prefix || path.starts_with(&format!("{}/", prefix)))
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

/// Split command line on |, ;, & but respect quoted strings.
/// Fixes issues where jq expressions like '.foo | .bar' were incorrectly split.
fn split_command_segments(cmd: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut chars = cmd.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
                current.push(c);
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
                current.push(c);
            }
            '\\' => {
                current.push(c);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '|' | ';' | '&' if !in_single_quote && !in_double_quote => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    segments.push(trimmed);
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        segments.push(trimmed);
    }

    segments
}
