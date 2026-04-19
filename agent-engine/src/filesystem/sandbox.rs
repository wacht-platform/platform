use common::error::AppError;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;

const MAX_CAPTURED_OUTPUT_BYTES: usize = 256 * 1024;
const NSJAIL_RLIMIT_AS_MB: u64 = 512;
const NSJAIL_RLIMIT_FSIZE_BYTES: u64 = 20 * 1024 * 1024;
const NSJAIL_RLIMIT_NOFILE: u64 = 128;
const NSJAIL_RLIMIT_NPROC: u64 = 128;
const NSJAIL_RLIMIT_STACK_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxFlavor {
    LinuxNsJail,
    MacSandboxExec,
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct SandboxOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug, Clone)]
pub struct SandboxExecutionOptions {
    pub allow_network: bool,
    pub writable_workspace: bool,
    pub writable_scratch: bool,
    pub extra_read_only_paths: Vec<PathBuf>,
    pub extra_env: Vec<(String, String)>,
}

impl Default for SandboxExecutionOptions {
    fn default() -> Self {
        Self {
            allow_network: false,
            writable_workspace: true,
            writable_scratch: true,
            extra_read_only_paths: Vec::new(),
            extra_env: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct SandboxRunner {
    execution_root: PathBuf,
    workspace_path: PathBuf,
    project_workspace_path: PathBuf,
    task_path: PathBuf,
    uploads_path: PathBuf,
    scratch_path: PathBuf,
    knowledge_path: PathBuf,
    skills_path: PathBuf,
    flavor: SandboxFlavor,
}

impl SandboxRunner {
    pub fn new(execution_root: PathBuf) -> Self {
        let resolve = |name: &str| {
            let path = execution_root.join(name);
            std::fs::canonicalize(&path).unwrap_or(path)
        };

        Self {
            workspace_path: resolve("workspace"),
            project_workspace_path: resolve("project_workspace"),
            task_path: resolve("task"),
            uploads_path: resolve("uploads"),
            scratch_path: resolve("scratch"),
            knowledge_path: resolve("knowledge"),
            skills_path: resolve("skills"),
            flavor: detect_sandbox_flavor(),
            execution_root,
        }
    }

    pub fn uses_virtual_alias_paths(&self) -> bool {
        matches!(self.flavor, SandboxFlavor::LinuxNsJail)
    }

    pub async fn execute_shell(
        &self,
        command_line: &str,
        timeout_secs: u64,
    ) -> Result<SandboxOutput, AppError> {
        self.execute_program_with_options(
            "/bin/bash",
            &["-c".to_string(), command_line.to_string()],
            timeout_secs,
            SandboxExecutionOptions::default(),
        )
        .await
    }

    pub async fn execute_program(
        &self,
        program: &str,
        args: &[String],
        timeout_secs: u64,
    ) -> Result<SandboxOutput, AppError> {
        self.execute_program_with_options(
            program,
            args,
            timeout_secs,
            SandboxExecutionOptions::default(),
        )
        .await
    }

    pub async fn execute_program_with_options(
        &self,
        program: &str,
        args: &[String],
        timeout_secs: u64,
        options: SandboxExecutionOptions,
    ) -> Result<SandboxOutput, AppError> {
        let output = match self.flavor {
            SandboxFlavor::LinuxNsJail => {
                self.execute_with_nsjail(program, args, timeout_secs, options)
                    .await?
            }
            SandboxFlavor::MacSandboxExec => {
                self.execute_with_sandbox_exec(program, args, timeout_secs, options)
                    .await?
            }
            SandboxFlavor::Unsupported => {
                return Err(AppError::Internal(
                    "No supported sandbox backend found. Install nsjail (Linux) or sandbox-exec (macOS)."
                        .to_string(),
                ))
            }
        };

        Ok(output)
    }

    pub async fn execute_program_with_input_and_options(
        &self,
        program: &str,
        args: &[String],
        input: Option<&[u8]>,
        timeout_secs: u64,
        options: SandboxExecutionOptions,
    ) -> Result<SandboxOutput, AppError> {
        let output = match self.flavor {
            SandboxFlavor::LinuxNsJail => {
                self.execute_with_nsjail_input(program, args, input, timeout_secs, options)
                    .await?
            }
            SandboxFlavor::MacSandboxExec => {
                self.execute_with_sandbox_exec_input(program, args, input, timeout_secs, options)
                    .await?
            }
            SandboxFlavor::Unsupported => {
                return Err(AppError::Internal(
                    "No supported sandbox backend found. Install nsjail (Linux) or sandbox-exec (macOS)."
                        .to_string(),
                ))
            }
        };

        Ok(output)
    }

    async fn execute_with_nsjail(
        &self,
        program: &str,
        args: &[String],
        timeout_secs: u64,
        options: SandboxExecutionOptions,
    ) -> Result<SandboxOutput, AppError> {
        self.ensure_required_paths()?;

        let mut cmd = Command::new("nsjail");
        cmd.env_clear();
        cmd.env("PATH", "/usr/bin:/bin");
        cmd.env("LANG", "C.UTF-8");
        cmd.env("LC_ALL", "C.UTF-8");
        cmd.env("HOME", "/tmp");
        cmd.env("TMPDIR", "/tmp");
        for (key, value) in &options.extra_env {
            cmd.env(key, value);
        }

        cmd.args([
            "--mode",
            "o",
            "--quiet",
            "--time_limit",
            &timeout_secs.to_string(),
            "--cwd",
            "/workspace",
            "--disable_proc",
            "--rlimit_as",
            &(NSJAIL_RLIMIT_AS_MB * 1024 * 1024).to_string(),
            "--rlimit_cpu",
            &timeout_secs.max(1).to_string(),
            "--rlimit_fsize",
            &NSJAIL_RLIMIT_FSIZE_BYTES.to_string(),
            "--rlimit_nofile",
            &NSJAIL_RLIMIT_NOFILE.to_string(),
            "--rlimit_nproc",
            &NSJAIL_RLIMIT_NPROC.to_string(),
            "--rlimit_stack",
            &NSJAIL_RLIMIT_STACK_BYTES.to_string(),
        ]);
        add_nsjail_flag_if_supported(&mut cmd, "--clone_newipc");
        add_nsjail_flag_if_supported(&mut cmd, "--clone_newuts");
        add_nsjail_flag_if_supported(&mut cmd, "--clone_newpid");
        add_nsjail_flag_if_supported(&mut cmd, "--clone_newns");

        if !options.allow_network {
            add_nsjail_flag_if_supported(&mut cmd, "--iface_no_lo");
            add_nsjail_flag_if_supported(&mut cmd, "--clone_newnet");
        }

        for path in ["/usr", "/bin"] {
            add_nsjail_ro_bind(&mut cmd, path, path);
        }
        for path in ["/lib", "/lib64"] {
            add_optional_nsjail_ro_bind(&mut cmd, path);
        }

        add_nsjail_ro_bind(
            &mut cmd,
            self.execution_root.to_string_lossy().as_ref(),
            "/app",
        );
        add_bind_for_access(
            &mut cmd,
            self.workspace_path.as_path(),
            "/workspace",
            options.writable_workspace,
        );
        add_bind_for_access(
            &mut cmd,
            self.project_workspace_path.as_path(),
            "/project_workspace",
            false,
        );
        add_bind_for_access(&mut cmd, self.task_path.as_path(), "/task", true);
        add_bind_for_access(
            &mut cmd,
            self.scratch_path.as_path(),
            "/scratch",
            options.writable_scratch,
        );
        add_optional_nsjail_ro_bind_path(&mut cmd, self.uploads_path.as_path(), "/uploads");
        add_optional_nsjail_ro_bind_path(&mut cmd, self.knowledge_path.as_path(), "/knowledge");
        add_optional_nsjail_ro_bind_path(&mut cmd, self.skills_path.as_path(), "/skills");
        add_extra_nsjail_ro_binds(&mut cmd, &options.extra_read_only_paths);
        add_nsjail_tmpfs(&mut cmd, "/tmp");

        cmd.arg("--");
        cmd.arg(program);

        for arg in args {
            cmd.arg(arg);
        }

        run_with_timeout(cmd, timeout_secs).await
    }

    async fn execute_with_nsjail_input(
        &self,
        program: &str,
        args: &[String],
        input: Option<&[u8]>,
        timeout_secs: u64,
        options: SandboxExecutionOptions,
    ) -> Result<SandboxOutput, AppError> {
        self.ensure_required_paths()?;

        let mut cmd = Command::new("nsjail");
        cmd.env_clear();
        cmd.env("PATH", "/usr/bin:/bin");
        cmd.env("LANG", "C.UTF-8");
        cmd.env("LC_ALL", "C.UTF-8");
        cmd.env("HOME", "/tmp");
        cmd.env("TMPDIR", "/tmp");
        for (key, value) in &options.extra_env {
            cmd.env(key, value);
        }

        cmd.args([
            "--mode",
            "o",
            "--quiet",
            "--time_limit",
            &timeout_secs.to_string(),
            "--cwd",
            "/workspace",
            "--disable_proc",
            "--rlimit_as",
            &(NSJAIL_RLIMIT_AS_MB * 1024 * 1024).to_string(),
            "--rlimit_cpu",
            &timeout_secs.max(1).to_string(),
            "--rlimit_fsize",
            &NSJAIL_RLIMIT_FSIZE_BYTES.to_string(),
            "--rlimit_nofile",
            &NSJAIL_RLIMIT_NOFILE.to_string(),
            "--rlimit_nproc",
            &NSJAIL_RLIMIT_NPROC.to_string(),
            "--rlimit_stack",
            &NSJAIL_RLIMIT_STACK_BYTES.to_string(),
        ]);
        add_nsjail_flag_if_supported(&mut cmd, "--clone_newipc");
        add_nsjail_flag_if_supported(&mut cmd, "--clone_newuts");
        add_nsjail_flag_if_supported(&mut cmd, "--clone_newpid");
        add_nsjail_flag_if_supported(&mut cmd, "--clone_newns");

        if !options.allow_network {
            add_nsjail_flag_if_supported(&mut cmd, "--iface_no_lo");
            add_nsjail_flag_if_supported(&mut cmd, "--clone_newnet");
        }

        for path in ["/usr", "/bin"] {
            add_nsjail_ro_bind(&mut cmd, path, path);
        }
        for path in ["/lib", "/lib64"] {
            add_optional_nsjail_ro_bind(&mut cmd, path);
        }

        add_nsjail_ro_bind(
            &mut cmd,
            self.execution_root.to_string_lossy().as_ref(),
            "/app",
        );
        add_bind_for_access(
            &mut cmd,
            self.workspace_path.as_path(),
            "/workspace",
            options.writable_workspace,
        );
        add_bind_for_access(
            &mut cmd,
            self.project_workspace_path.as_path(),
            "/project_workspace",
            false,
        );
        add_bind_for_access(&mut cmd, self.task_path.as_path(), "/task", true);
        add_bind_for_access(
            &mut cmd,
            self.scratch_path.as_path(),
            "/scratch",
            options.writable_scratch,
        );
        add_optional_nsjail_ro_bind_path(&mut cmd, self.uploads_path.as_path(), "/uploads");
        add_optional_nsjail_ro_bind_path(&mut cmd, self.knowledge_path.as_path(), "/knowledge");
        add_optional_nsjail_ro_bind_path(&mut cmd, self.skills_path.as_path(), "/skills");
        add_extra_nsjail_ro_binds(&mut cmd, &options.extra_read_only_paths);
        add_nsjail_tmpfs(&mut cmd, "/tmp");

        cmd.arg("--");
        cmd.arg(program);
        for arg in args {
            cmd.arg(arg);
        }

        run_with_timeout_and_input(cmd, input, timeout_secs).await
    }

    async fn execute_with_sandbox_exec(
        &self,
        program: &str,
        args: &[String],
        timeout_secs: u64,
        options: SandboxExecutionOptions,
    ) -> Result<SandboxOutput, AppError> {
        let workspace = escape_for_sandbox_profile(self.workspace_path.as_path());
        let project_workspace = escape_for_sandbox_profile(self.project_workspace_path.as_path());
        let task = escape_for_sandbox_profile(self.task_path.as_path());
        let scratch = escape_for_sandbox_profile(self.scratch_path.as_path());
        let network_rule = if options.allow_network {
            "             (allow network*)\n".to_string()
        } else {
            "             (deny network*)\n".to_string()
        };
        let workspace_write_rule = if options.writable_workspace {
            format!(" (subpath \"{workspace}\")")
        } else {
            String::new()
        };
        let scratch_write_rule = if options.writable_scratch {
            format!(" (subpath \"{scratch}\")")
        } else {
            String::new()
        };
        let task_write_rule = format!(" (subpath \"{task}\")");

        let profile = format!(
            "(version 1)\n\
             (deny default)\n\
             (allow process*)\n\
             (allow sysctl-read)\n\
             (allow file-read*)\n\
             (allow file-read* (subpath \"{project_workspace}\"))\n\
             (allow file-write*{workspace_write_rule}{task_write_rule}{scratch_write_rule} (subpath \"/tmp\") (subpath \"/private/tmp\"))\n\
             {network_rule}"
        );

        let mut cmd = Command::new("sandbox-exec");
        cmd.arg("-p").arg(profile).arg(program);
        for (key, value) in &options.extra_env {
            cmd.env(key, value);
        }
        for arg in args {
            cmd.arg(arg);
        }
        cmd.current_dir(&self.workspace_path);

        run_with_timeout(cmd, timeout_secs).await
    }

    async fn execute_with_sandbox_exec_input(
        &self,
        program: &str,
        args: &[String],
        input: Option<&[u8]>,
        timeout_secs: u64,
        options: SandboxExecutionOptions,
    ) -> Result<SandboxOutput, AppError> {
        let workspace = escape_for_sandbox_profile(self.workspace_path.as_path());
        let project_workspace = escape_for_sandbox_profile(self.project_workspace_path.as_path());
        let task = escape_for_sandbox_profile(self.task_path.as_path());
        let scratch = escape_for_sandbox_profile(self.scratch_path.as_path());
        let network_rule = if options.allow_network {
            "             (allow network*)\n".to_string()
        } else {
            "             (deny network*)\n".to_string()
        };
        let workspace_write_rule = if options.writable_workspace {
            format!(" (subpath \"{workspace}\")")
        } else {
            String::new()
        };
        let scratch_write_rule = if options.writable_scratch {
            format!(" (subpath \"{scratch}\")")
        } else {
            String::new()
        };
        let task_write_rule = format!(" (subpath \"{task}\")");

        let profile = format!(
            "(version 1)\n\
             (deny default)\n\
             (allow process*)\n\
             (allow sysctl-read)\n\
             (allow file-read*)\n\
             (allow file-read* (subpath \"{project_workspace}\"))\n\
             (allow file-write*{workspace_write_rule}{task_write_rule}{scratch_write_rule} (subpath \"/tmp\") (subpath \"/private/tmp\"))\n\
             {network_rule}"
        );

        let mut cmd = Command::new("sandbox-exec");
        cmd.arg("-p").arg(profile).arg(program);
        for (key, value) in &options.extra_env {
            cmd.env(key, value);
        }
        for arg in args {
            cmd.arg(arg);
        }
        cmd.current_dir(&self.workspace_path);

        run_with_timeout_and_input(cmd, input, timeout_secs).await
    }
}

fn add_nsjail_ro_bind(cmd: &mut Command, source: &str, target: &str) {
    cmd.arg("--bindmount_ro")
        .arg(format!("{}:{}", source, target));
}

fn add_nsjail_rw_bind(cmd: &mut Command, source: &Path, target: &str) {
    cmd.arg("--bindmount")
        .arg(format!("{}:{}", source.display(), target));
}

fn add_nsjail_tmpfs(cmd: &mut Command, target: &str) {
    cmd.args(["--tmpfsmount", target]);
}

fn add_optional_nsjail_ro_bind(cmd: &mut Command, path: &str) {
    if Path::new(path).exists() {
        add_nsjail_ro_bind(cmd, path, path);
    }
}

fn add_optional_nsjail_ro_bind_path(cmd: &mut Command, source: &Path, target: &str) {
    if source.exists() {
        cmd.arg("--bindmount_ro")
            .arg(format!("{}:{}", source.display(), target));
    }
}

fn add_bind_for_access(cmd: &mut Command, source: &Path, target: &str, writable: bool) {
    if writable {
        add_nsjail_rw_bind(cmd, source, target);
    } else {
        cmd.arg("--bindmount_ro")
            .arg(format!("{}:{}", source.display(), target));
    }
}

fn add_extra_nsjail_ro_binds(cmd: &mut Command, paths: &[PathBuf]) {
    for path in canonicalized_existing_paths(paths) {
        add_nsjail_ro_bind(
            cmd,
            path.to_string_lossy().as_ref(),
            path.to_string_lossy().as_ref(),
        );
    }
}

fn escape_for_sandbox_profile(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn canonicalized_existing_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut resolved = Vec::new();

    for path in paths {
        if !path.exists() {
            continue;
        }

        match std::fs::canonicalize(path) {
            Ok(canonical) => resolved.push(canonical),
            Err(_) => resolved.push(path.clone()),
        }
    }

    resolved.sort();
    resolved.dedup();
    resolved
}

async fn run_with_timeout(mut cmd: Command, timeout_secs: u64) -> Result<SandboxOutput, AppError> {
    let output = timeout(Duration::from_secs(timeout_secs), cmd.output())
        .await
        .map_err(|_| AppError::Timeout)?
        .map_err(|e| AppError::Internal(format!("Sandbox execution failed: {}", e)))?;

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        let _ = output.status.signal();
    }

    #[cfg(not(unix))]
    {}

    Ok(SandboxOutput {
        stdout: truncate_output(&output.stdout),
        stderr: truncate_output(&output.stderr),
        exit_code: output.status.code().unwrap_or(-1),
    })
}

async fn run_with_timeout_and_input(
    mut cmd: Command,
    input: Option<&[u8]>,
    timeout_secs: u64,
) -> Result<SandboxOutput, AppError> {
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Internal(format!("Sandbox execution failed: {}", e)))?;

    if let Some(bytes) = input {
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(bytes)
                .await
                .map_err(|e| AppError::Internal(format!("Failed to write sandbox stdin: {}", e)))?;
        }
    }

    let output = timeout(Duration::from_secs(timeout_secs), child.wait_with_output())
        .await
        .map_err(|_| AppError::Timeout)?
        .map_err(|e| AppError::Internal(format!("Sandbox execution failed: {}", e)))?;

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        let _ = output.status.signal();
    }

    #[cfg(not(unix))]
    {}

    Ok(SandboxOutput {
        stdout: truncate_output(&output.stdout),
        stderr: truncate_output(&output.stderr),
        exit_code: output.status.code().unwrap_or(-1),
    })
}

fn detect_sandbox_flavor() -> SandboxFlavor {
    if cfg!(target_os = "linux") && command_exists("nsjail") {
        return SandboxFlavor::LinuxNsJail;
    }
    if cfg!(target_os = "macos") && command_exists("sandbox-exec") {
        return SandboxFlavor::MacSandboxExec;
    }
    SandboxFlavor::Unsupported
}

fn command_exists(name: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {} >/dev/null 2>&1", name))
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn add_nsjail_flag_if_supported(cmd: &mut Command, flag: &'static str) {
    if nsjail_supports_flag(flag) {
        cmd.arg(flag);
    }
}

fn nsjail_supports_flag(flag: &'static str) -> bool {
    static NSJAIL_FLAGS: OnceLock<std::collections::HashSet<String>> = OnceLock::new();
    let flags = NSJAIL_FLAGS.get_or_init(detect_nsjail_flags);
    flags.contains(flag)
}

fn detect_nsjail_flags() -> std::collections::HashSet<String> {
    let output = std::process::Command::new("nsjail").arg("--help").output();
    let text = match output {
        Ok(output) => format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(_) => String::new(),
    };

    text.split_whitespace()
        .filter_map(|token| {
            if !token.starts_with("--") {
                return None;
            }
            let normalized = token
                .trim_end_matches(|ch: char| {
                    ch == ','
                        || ch == ';'
                        || ch == ':'
                        || ch == ')'
                        || ch == ']'
                        || ch == '}'
                })
                .split('=')
                .next()
                .unwrap_or(token)
                .to_string();
            Some(normalized)
        })
        .collect()
}

fn truncate_output(bytes: &[u8]) -> String {
    if bytes.len() <= MAX_CAPTURED_OUTPUT_BYTES {
        return String::from_utf8_lossy(bytes).to_string();
    }

    let truncated = &bytes[..MAX_CAPTURED_OUTPUT_BYTES];
    format!(
        "{}\n[output truncated at {} bytes]",
        String::from_utf8_lossy(truncated),
        MAX_CAPTURED_OUTPUT_BYTES
    )
}

impl SandboxRunner {
    fn ensure_required_paths(&self) -> Result<(), AppError> {
        std::fs::create_dir_all(&self.execution_root).map_err(|e| {
            AppError::Internal(format!(
                "Failed to prepare sandbox execution root '{}': {}",
                self.execution_root.display(),
                e
            ))
        })?;
        std::fs::create_dir_all(&self.workspace_path).map_err(|e| {
            AppError::Internal(format!(
                "Failed to prepare sandbox workspace '{}': {}",
                self.workspace_path.display(),
                e
            ))
        })?;
        std::fs::create_dir_all(&self.project_workspace_path).map_err(|e| {
            AppError::Internal(format!(
                "Failed to prepare sandbox project workspace '{}': {}",
                self.project_workspace_path.display(),
                e
            ))
        })?;
        std::fs::create_dir_all(&self.task_path).map_err(|e| {
            AppError::Internal(format!(
                "Failed to prepare sandbox task mount '{}': {}",
                self.task_path.display(),
                e
            ))
        })?;
        std::fs::create_dir_all(&self.scratch_path).map_err(|e| {
            AppError::Internal(format!(
                "Failed to prepare sandbox scratch '{}': {}",
                self.scratch_path.display(),
                e
            ))
        })?;
        std::fs::create_dir_all(&self.skills_path).map_err(|e| {
            AppError::Internal(format!(
                "Failed to prepare sandbox skills mount '{}': {}",
                self.skills_path.display(),
                e
            ))
        })?;
        Ok(())
    }
}
