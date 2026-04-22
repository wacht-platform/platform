use crate::filesystem::sandbox::SandboxRunner;
use common::error::AppError;
use serde::{Deserialize, Serialize};
use shellish_parse::{multiparse, ParseOptions};
use std::env;
use std::path::Path;
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

#[derive(Debug, Clone)]
struct ParsedCommandStage {
    env: Vec<(String, String)>,
    program: String,
    args: Vec<String>,
    next_separator: CommandSeparator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandSeparator {
    And,
    Or,
    Sequence,
    Pipe,
    End,
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
                "wc",
                "mkdir",
                "cp",
                "mv",
                "rm",
                "sed",
                "awk",
                "sort",
                "uniq",
                "jq",
                "cut",
                "tr",
                "diff",
                "which",
                "pdftotext",
                "pdftoppm",
                "pdfinfo",
                "file",
                "python",
                "python3",
                "whoami",
                "id",
                "pwd",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        }
    }

    pub async fn execute(&self, command_line: &str) -> Result<ShellOutput, AppError> {
        let stages = self.parse_command_line(command_line)?;
        let mut stdin_bytes: Option<Vec<u8>> = None;
        let mut combined_stderr = String::new();
        let mut last_stdout = String::new();
        let mut last_exit_code = 0;
        let mut previous_separator = CommandSeparator::Sequence;
        let mut has_executed_stage = false;

        for stage in stages {
            let should_execute = if !has_executed_stage {
                true
            } else {
                match previous_separator {
                    CommandSeparator::And => last_exit_code == 0,
                    CommandSeparator::Or => last_exit_code != 0,
                    CommandSeparator::Sequence | CommandSeparator::Pipe | CommandSeparator::End => {
                        true
                    }
                }
            };

            let stage_separator = stage.next_separator;
            if !should_execute {
                stdin_bytes = None;
                previous_separator = stage_separator;
                continue;
            }

            let mut options = shell_execution_options();
            options.extra_env.extend(stage.env.clone());

            let resolved_program = resolve_program_path(&stage.program, &options)?;

            let output = self
                .sandbox
                .execute_program_with_input_and_options(
                    &resolved_program,
                    &stage.args,
                    if previous_separator == CommandSeparator::Pipe {
                        stdin_bytes.as_deref()
                    } else {
                        None
                    },
                    self.timeout_secs,
                    options,
                )
                .await?;
            has_executed_stage = true;

            if !output.stderr.is_empty() {
                if !combined_stderr.is_empty() {
                    combined_stderr.push('\n');
                }
                combined_stderr.push_str(&output.stderr);
            }

            last_exit_code = output.exit_code;
            last_stdout = output.stdout.clone();

            if stage_separator == CommandSeparator::Pipe {
                stdin_bytes = Some(output.stdout.into_bytes());
            } else {
                stdin_bytes = None;
            }

            previous_separator = stage_separator;
        }

        Ok(ShellOutput {
            stdout: last_stdout,
            stderr: combined_stderr,
            exit_code: last_exit_code,
        })
    }

    fn normalize_path_alias(&self, token: &str) -> String {
        if self.sandbox.uses_virtual_alias_paths() {
            return token.to_string();
        }

        let working_dir_str = self.working_dir.to_string_lossy().to_string();
        let base = working_dir_str.trim_end_matches('/');
        let aliases: Vec<(&str, String)> = vec![
            ("/knowledge/", format!("{}/knowledge/", base)),
            ("/knowledge", format!("{}/knowledge", base)),
            ("/skills/", format!("{}/skills/", base)),
            ("/skills", format!("{}/skills", base)),
            ("/uploads/", format!("{}/uploads/", base)),
            ("/uploads", format!("{}/uploads", base)),
            ("/scratch/", format!("{}/scratch/", base)),
            ("/scratch", format!("{}/scratch", base)),
            ("/workspace/", format!("{}/workspace/", base)),
            ("/workspace", format!("{}/workspace", base)),
            (
                "/project_workspace/",
                format!("{}/project_workspace/", base),
            ),
            ("/project_workspace", format!("{}/project_workspace", base)),
            ("/task/", format!("{}/task/", base)),
            ("/task", format!("{}/task", base)),
        ];

        for (alias, replacement) in aliases {
            if token.starts_with(alias) {
                return format!("{}{}", replacement, &token[alias.len()..]);
            }
        }

        token.to_string()
    }

    fn parse_command_line(&self, command_line: &str) -> Result<Vec<ParsedCommandStage>, AppError> {
        const SEPARATORS: &[&str] = &["&&", "||", ";", "&", "|"];

        let parsed = multiparse(command_line, ParseOptions::new(), SEPARATORS).map_err(|e| {
            AppError::BadRequest(format!("Invalid command syntax in execute_command: {}", e))
        })?;

        if parsed.is_empty() {
            return Err(AppError::BadRequest("Empty command".to_string()));
        }

        parsed
            .into_iter()
            .map(|(tokens, terminator)| {
                let next_separator = match terminator {
                    Some(0) => CommandSeparator::And,
                    Some(1) => CommandSeparator::Or,
                    Some(2) => CommandSeparator::Sequence,
                    Some(3) => {
                        return Err(AppError::Forbidden(
                            "Background execution '&' is not allowed.".to_string(),
                        ))
                    }
                    Some(4) => CommandSeparator::Pipe,
                    None => CommandSeparator::End,
                    Some(_) => {
                        return Err(AppError::Forbidden(
                            "Unsupported shell separator in command.".to_string(),
                        ))
                    }
                };
                self.parse_command_stage(tokens, next_separator)
            })
            .collect()
    }

    fn parse_command_stage(
        &self,
        tokens: Vec<String>,
        next_separator: CommandSeparator,
    ) -> Result<ParsedCommandStage, AppError> {
        if tokens.is_empty() {
            return Err(AppError::BadRequest(
                "Empty command stage in pipeline".to_string(),
            ));
        }

        let mut env = Vec::new();
        let mut idx = 0usize;
        while idx < tokens.len() && is_env_assignment(&tokens[idx]) {
            let (key, value) = parse_env_assignment(&tokens[idx])?;
            env.push((key, value));
            idx += 1;
        }

        if idx >= tokens.len() {
            return Err(AppError::BadRequest(
                "Command stage contains only environment assignments with no program".to_string(),
            ));
        }

        let program = self.normalize_path_alias(&tokens[idx]);
        let program_name = program
            .rsplit('/')
            .next()
            .unwrap_or(program.as_str())
            .to_string();

        if !self.allowed_commands.contains(&program_name) {
            return Err(AppError::Forbidden(format!(
                "Command '{}' is not allowed",
                program_name
            )));
        }

        let mut args = Vec::new();
        for token in tokens.into_iter().skip(idx + 1) {
            let normalized = self.normalize_path_alias(&token);
            validate_token_syntax(&normalized)?;
            validate_token_path(&normalized)?;
            args.push(normalized);
        }

        Ok(ParsedCommandStage {
            env,
            program,
            args,
            next_separator,
        })
    }
}

fn shell_execution_options() -> crate::filesystem::sandbox::SandboxExecutionOptions {
    let mut options = crate::filesystem::sandbox::SandboxExecutionOptions::default();

    let python_path = shell_code_runner_python_path();
    if let Some(env_root) = shell_code_runner_env_root(&python_path) {
        let bin_dir = env_root.join("bin");
        let path_value = format!("{}:/usr/bin:/bin", bin_dir.display());
        options.extra_env.push(("PATH".to_string(), path_value));
        options.extra_env.push((
            "VIRTUAL_ENV".to_string(),
            env_root.to_string_lossy().to_string(),
        ));
        options.extra_read_only_paths.push(env_root);
    }

    options
}

fn shell_code_runner_python_path() -> PathBuf {
    if let Ok(path) = env::var("CODE_RUNNER_PYTHON_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    if cfg!(target_os = "macos") {
        return PathBuf::from(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("agent-engine crate should live under platform-api")
                .join(".runtime/code_runner/venv/bin/python"),
        );
    }

    PathBuf::from("/opt/wacht/code_runner/venv/bin/python")
}

fn shell_code_runner_env_root(python_path: &Path) -> Option<PathBuf> {
    let bin_dir = python_path.parent()?;
    let env_root = bin_dir.parent()?;
    Some(env_root.to_path_buf())
}

fn is_allowed_absolute_path(path: &str) -> bool {
    [
        "/knowledge",
        "/skills",
        "/uploads",
        "/scratch",
        "/workspace",
        "/project_workspace",
        "/task",
        "/app",
        "/tmp",
    ]
    .iter()
    .any(|prefix| path == *prefix || path.starts_with(&format!("{}/", prefix)))
}

fn is_env_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn parse_env_assignment(token: &str) -> Result<(String, String), AppError> {
    let (name, value) = token.split_once('=').ok_or_else(|| {
        AppError::BadRequest(format!("Invalid environment assignment '{}'", token))
    })?;
    Ok((name.to_string(), value.to_string()))
}

fn validate_token_path(token: &str) -> Result<(), AppError> {
    if token == "//" {
        return Ok(());
    }
    if contains_path_traversal(token) {
        return Err(AppError::Forbidden(
            "Path traversal (..) is not allowed in commands".to_string(),
        ));
    }
    if token.starts_with('/') && !is_allowed_absolute_path(token) {
        return Err(AppError::Forbidden(format!(
            "Absolute path '{}' is outside allowed directories. Use /knowledge/, /skills/, /uploads/, /scratch/, /workspace/, /project_workspace/, /task/, or /app/",
            token
        )));
    }
    Ok(())
}

/// Traversal means `..` occupies a whole path segment (delimited by `/` or `\`).
/// Double-dots inside a filename — `Resume..pdf`, `file..tar.gz` — are NOT traversal
/// and must be allowed through.
fn contains_path_traversal(token: &str) -> bool {
    token
        .split(|c: char| c == '/' || c == '\\')
        .any(|segment| segment == "..")
}

fn validate_token_syntax(token: &str) -> Result<(), AppError> {
    if token.contains('`') {
        return Err(AppError::Forbidden(
            "Backtick command substitution is not allowed".to_string(),
        ));
    }
    if token.contains("$(") {
        return Err(AppError::Forbidden(
            "Command substitution '$(...)' is not allowed".to_string(),
        ));
    }
    if token.contains('>') || token.contains('<') {
        return Err(AppError::Forbidden(
            "Redirection syntax is not allowed".to_string(),
        ));
    }
    Ok(())
}

fn resolve_program_path(
    program: &str,
    options: &crate::filesystem::sandbox::SandboxExecutionOptions,
) -> Result<String, AppError> {
    if program.contains('/') {
        return Ok(program.to_string());
    }

    let mut search_paths = Vec::<String>::new();
    if let Some(path_value) = options
        .extra_env
        .iter()
        .find_map(|(key, value)| (key == "PATH").then_some(value.clone()))
    {
        search_paths.extend(path_value.split(':').map(|s| s.to_string()));
    } else {
        search_paths.push("/usr/bin".to_string());
        search_paths.push("/bin".to_string());
    }

    if !search_paths.iter().any(|path| path == "/usr/bin") {
        search_paths.push("/usr/bin".to_string());
    }
    if !search_paths.iter().any(|path| path == "/bin") {
        search_paths.push("/bin".to_string());
    }

    for path in search_paths {
        if path.trim().is_empty() {
            continue;
        }
        let candidate = Path::new(&path).join(program);
        if candidate.is_file() {
            return Ok(candidate.to_string_lossy().to_string());
        }
    }

    Err(AppError::Forbidden(format!(
        "Command '{}' was allowed but no executable was found in sandbox PATH",
        program
    )))
}
