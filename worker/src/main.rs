use anyhow::{Context, Result, bail};
use common::state::AppState;
use dotenvy::dotenv;
use tokio::process::Command;
use tracing::info;

mod consumer;
mod jobs;
mod scheduler;
mod tasks;
mod throttler;

async fn ensure_rclone_available() -> Result<()> {
    let version_output = Command::new("rclone")
        .arg("version")
        .output()
        .await
        .context("failed to execute `rclone`; install it and ensure it is available on PATH")?;

    if !version_output.status.success() {
        let stderr = String::from_utf8_lossy(&version_output.stderr);
        bail!(
            "`rclone version` failed during worker startup: {}",
            stderr.trim()
        );
    }

    let nfsmount_output = Command::new("rclone")
        .arg("nfsmount")
        .arg("--help")
        .output()
        .await
        .context("failed to execute `rclone nfsmount`; install a build with nfsmount support")?;

    if !nfsmount_output.status.success() {
        let stderr = String::from_utf8_lossy(&nfsmount_output.stderr);
        bail!(
            "`rclone nfsmount --help` failed during worker startup: {}",
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&version_output.stdout);
    let version_line = stdout.lines().next().unwrap_or("rclone");
    info!(version = %version_line, "Verified rclone nfsmount availability for worker startup");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    common::init_telemetry("platform-worker")
        .map_err(|e| anyhow::anyhow!("failed to initialize telemetry: {e}"))?;
    ensure_rclone_available().await?;
    agent_engine::filesystem::mounts::cleanup_startup_mounts().await?;

    let app_state = AppState::new_from_env().await.unwrap();

    let scheduler = scheduler::JobScheduler::new(app_state.clone());
    scheduler.start().await?;

    let consumer = consumer::NatsConsumer::new(app_state.clone()).await?;
    consumer.start_consuming().await?;

    Ok(())
}
