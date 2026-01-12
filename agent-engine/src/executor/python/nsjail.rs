use super::{ExecutionResult, PythonExecutor};
use async_trait::async_trait;
use common::error::AppError;
use std::process::Stdio;
use std::time::Instant;

pub struct NsJailExecutor {
    python_path: String,
}

impl NsJailExecutor {
    pub fn new() -> Self {
        Self {
            python_path: "/usr/bin/python3".to_string(),
        }
    }

    async fn generate_config(
        &self,
        config_path: &std::path::Path,
        execution_root: &std::path::Path,
    ) -> Result<(), AppError> {
        let exec_root_str = execution_root.to_string_lossy();
        
        // Resolve symlinks to actual paths - bind mounts don't follow symlinks!
        // workspace and uploads are symlinks to persistent storage, we need the real paths.
        let resolve_path = |subdir: &str| -> String {
            let path = execution_root.join(subdir);
            std::fs::canonicalize(&path)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| path.to_string_lossy().to_string())
        };
        
        let workspace_path = resolve_path("workspace");
        let uploads_path = resolve_path("uploads");
        let scratch_path = resolve_path("scratch");
        let knowledge_path = resolve_path("knowledge");
        let teams_activity_path = resolve_path("teams-activity");
        
        // Minimal configuration for running Python
        // We mount /usr, /lib, /lib64, /bin because host python relies on them used shared libs.
        // We strictly bind-mount the execution root.
        // We also mount alias paths so scripts can use /teams-activity/, /knowledge/, etc.
        let config = format!(
            r#"
name: "agent-python-sandbox"
mode: ONCE
hostname: "agent-sandbox"
cwd: "/app"

time_limit: 30
rlimit_as: 512
rlimit_cpu: 30
rlimit_fsize: 10

clone_newnet: true
clone_newuser: true
clone_newns: true
clone_newpid: true
clone_newipc: true
clone_newuts: true
clone_newcgroup: true

uidmap {{
    inside_id: "0"
    outside_id: "0"
    count: 1
}}

gidmap {{
    inside_id: "0"
    outside_id: "0"
    count: 1
}}

# Minimal System Mounts
mount {{
    src: "/usr/lib"
    dst: "/usr/lib"
    is_bind: true
    rw: false
}}

mount {{
    src: "/usr/local/lib"
    dst: "/usr/local/lib"
    is_bind: true
    rw: false
    mandatory: false
}}

mount {{
    src: "/lib"
    dst: "/lib"
    is_bind: true
    rw: false
}}

mount {{
    src: "/lib64"
    dst: "/lib64"
    is_bind: true
    rw: false
    mandatory: false
}}

mount {{
    src: "/usr/bin/python3"
    dst: "/usr/bin/python3"
    is_bind: true
    rw: false
}}
# Symlink for python -> python3 if needed
mount {{
    src: "/usr/bin/python3"
    dst: "/usr/bin/python"
    is_bind: true
    rw: false
    mandatory: false
}}

mount {{
    dst: "/tmp"
    fstype: "tmpfs"
    rw: true
    options: "size=64m"
}}

mount {{
    dst: "/proc"
    fstype: "proc"
    rw: false
}}

mount {{
    src: "/dev/null"
    dst: "/dev/null"
    is_bind: true
    rw: true
}}

# Execution Root Bind - main workspace at /app
mount {{
    src: "{exec_root}"
    dst: "/app"
    is_bind: true
    rw: true
}}

# Alias path mounts - so scripts can use the same paths as the agent
# These allow Python to use /teams-activity/, /knowledge/, etc. directly
# Using resolved paths (not symlinks) since bind mounts don't follow symlinks
mount {{
    src: "{teams_activity}"
    dst: "/teams-activity"
    is_bind: true
    rw: true
    mandatory: false
}}

mount {{
    src: "{knowledge}"
    dst: "/knowledge"
    is_bind: true
    rw: false
    mandatory: false
}}

mount {{
    src: "{uploads}"
    dst: "/uploads"
    is_bind: true
    rw: false
    mandatory: false
}}

mount {{
    src: "{scratch}"
    dst: "/scratch"
    is_bind: true
    rw: true
    mandatory: false
}}

mount {{
    src: "{workspace}"
    dst: "/workspace"
    is_bind: true
    rw: true
    mandatory: false
}}

# Full execution root path mount - fallback if agent uses actual path
# This allows scripts using /mnt/wacht-agents/... directly to still work
mount {{
    src: "{exec_root}"
    dst: "{exec_root}"
    is_bind: true
    rw: true
}}
"#,
            exec_root = exec_root_str,
            workspace = workspace_path,
            uploads = uploads_path,
            scratch = scratch_path,
            knowledge = knowledge_path,
            teams_activity = teams_activity_path
        );

        tokio::fs::write(config_path, config)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to write nsjail config: {}", e)))?;

        Ok(())
    }
}

#[async_trait]
impl PythonExecutor for NsJailExecutor {
    async fn execute_script(
        &self,
        execution_root: &std::path::Path,
        script_path: &std::path::Path,
        args: Vec<String>,
        timeout_secs: u64,
    ) -> Result<ExecutionResult, AppError> {
        let start_time = Instant::now();

        let scratch_dir = execution_root.join("scratch");
        if !scratch_dir.exists() {
            tokio::fs::create_dir_all(&scratch_dir)
                .await
                .map_err(|e| AppError::Internal(format!("Failed to ensure scratch dir: {}", e)))?;
        }

        let config_name = "nsjail.cfg";
        let config_path = scratch_dir.join(config_name);

        self.generate_config(&config_path, execution_root).await?;

        let mut cmd = tokio::process::Command::new("nsjail");

        cmd.arg("--config").arg(&config_path);

        cmd.arg("--time_limit").arg(timeout_secs.to_string());

        cmd.arg("--");
        cmd.arg(&self.python_path);

        let script_relative = script_path.to_string_lossy();
        
        let script_inside_jail = if script_relative.starts_with("/workspace/")
            || script_relative.starts_with("/scratch/")
            || script_relative.starts_with("/knowledge/")
            || script_relative.starts_with("/uploads/")
            || script_relative.starts_with("/teams-activity/")
            || script_relative.starts_with("/app/")
        {
            script_relative.to_string()
        } else if script_relative.starts_with("./") {
            format!("/app/{}", &script_relative[2..])
        } else if script_relative.starts_with("/") {
            script_relative.to_string()
        } else {
            format!("/app/{}", script_relative)
        };

        cmd.arg(script_inside_jail);
        cmd.args(&args);

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let child_res = cmd.spawn();

        if let Err(e) = child_res {
            let _ = tokio::fs::remove_file(&config_path).await;
            return Err(AppError::Internal(format!("Failed to spawn nsjail: {}", e)));
        }
        let child = child_res.unwrap();

        let output_res = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            child.wait_with_output(),
        )
        .await;

        let _ = tokio::fs::remove_file(&config_path).await;

        let output = match output_res {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                return Err(AppError::Internal(format!(
                    "NsJail execution failed: {}",
                    e
                )))
            }
            Err(_) => {
                return Err(AppError::Internal(format!(
                    "Python script timed out after {}s",
                    timeout_secs
                )));
            }
        };

        Ok(ExecutionResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
            duration_ms: start_time.elapsed().as_millis(),
        })
    }
}
