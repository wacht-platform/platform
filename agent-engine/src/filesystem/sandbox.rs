use common::error::AppError;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxFlavor {
    LinuxBwrap,
    MacSandboxExec,
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct SandboxOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Clone)]
pub struct SandboxRunner {
    execution_root: PathBuf,
    workspace_path: PathBuf,
    uploads_path: PathBuf,
    scratch_path: PathBuf,
    knowledge_path: PathBuf,
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
            uploads_path: resolve("uploads"),
            scratch_path: resolve("scratch"),
            knowledge_path: resolve("knowledge"),
            flavor: detect_sandbox_flavor(),
            execution_root,
        }
    }

    pub fn flavor(&self) -> SandboxFlavor {
        self.flavor
    }

    pub fn uses_virtual_alias_paths(&self) -> bool {
        self.flavor == SandboxFlavor::LinuxBwrap
    }

    pub async fn execute_shell(
        &self,
        command_line: &str,
        timeout_secs: u64,
    ) -> Result<SandboxOutput, AppError> {
        self.execute_program(
            "/bin/bash",
            &["-lc".to_string(), command_line.to_string()],
            timeout_secs,
        )
        .await
    }

    pub async fn execute_program(
        &self,
        program: &str,
        args: &[String],
        timeout_secs: u64,
    ) -> Result<SandboxOutput, AppError> {
        let output = match self.flavor {
            SandboxFlavor::LinuxBwrap => self.execute_with_bwrap(program, args, timeout_secs).await?,
            SandboxFlavor::MacSandboxExec => {
                self.execute_with_sandbox_exec(program, args, timeout_secs)
                    .await?
            }
            SandboxFlavor::Unsupported => {
                return Err(AppError::Internal(
                    "No supported sandbox backend found. Install bwrap (Linux) or sandbox-exec (macOS)."
                        .to_string(),
                ))
            }
        };

        Ok(output)
    }

    async fn execute_with_bwrap(
        &self,
        program: &str,
        args: &[String],
        timeout_secs: u64,
    ) -> Result<SandboxOutput, AppError> {
        let mut cmd = Command::new("bwrap");

        cmd.args([
            "--die-with-parent",
            "--unshare-pid",
            "--unshare-net",
            "--unshare-ipc",
            "--unshare-uts",
            "--new-session",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--ro-bind",
            "/usr",
            "/usr",
            "--ro-bind",
            "/bin",
            "/bin",
        ]);

        add_optional_ro_bind(&mut cmd, "/lib");
        add_optional_ro_bind(&mut cmd, "/lib64");
        add_optional_ro_bind(&mut cmd, "/etc");

        cmd.args([
            "--bind",
            self.execution_root.to_string_lossy().as_ref(),
            "/app",
            "--bind",
            self.workspace_path.to_string_lossy().as_ref(),
            "/workspace",
            "--bind",
            self.scratch_path.to_string_lossy().as_ref(),
            "/scratch",
            "--ro-bind",
            self.uploads_path.to_string_lossy().as_ref(),
            "/uploads",
            "--ro-bind",
            self.knowledge_path.to_string_lossy().as_ref(),
            "/knowledge",
            "--chdir",
            "/app",
            "--tmpfs",
            "/tmp",
            "--",
            program,
        ]);

        for arg in args {
            cmd.arg(arg);
        }

        run_with_timeout(cmd, timeout_secs).await
    }

    async fn execute_with_sandbox_exec(
        &self,
        program: &str,
        args: &[String],
        timeout_secs: u64,
    ) -> Result<SandboxOutput, AppError> {
        let exec_root = escape_for_sandbox_profile(self.execution_root.as_path());
        let workspace = escape_for_sandbox_profile(self.workspace_path.as_path());
        let scratch = escape_for_sandbox_profile(self.scratch_path.as_path());

        let profile = format!(
            "(version 1)\n\
             (deny default)\n\
             (allow process*)\n\
             (allow file-read*)\n\
             (allow file-write* (subpath \"{exec_root}\") (subpath \"{workspace}\") (subpath \"{scratch}\") (subpath \"/tmp\") (subpath \"/private/tmp\"))\n\
             (deny network*)\n"
        );

        let mut cmd = Command::new("sandbox-exec");
        cmd.arg("-p").arg(profile).arg(program);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.current_dir(&self.execution_root);

        run_with_timeout(cmd, timeout_secs).await
    }
}

fn add_optional_ro_bind(cmd: &mut Command, path: &str) {
    if Path::new(path).exists() {
        cmd.args(["--ro-bind", path, path]);
    }
}

fn escape_for_sandbox_profile(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

async fn run_with_timeout(mut cmd: Command, timeout_secs: u64) -> Result<SandboxOutput, AppError> {
    let output = timeout(Duration::from_secs(timeout_secs), cmd.output())
        .await
        .map_err(|_| AppError::Timeout)?
        .map_err(|e| AppError::Internal(format!("Sandbox execution failed: {}", e)))?;

    Ok(SandboxOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}

fn detect_sandbox_flavor() -> SandboxFlavor {
    if cfg!(target_os = "linux") && command_exists("bwrap") {
        return SandboxFlavor::LinuxBwrap;
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
