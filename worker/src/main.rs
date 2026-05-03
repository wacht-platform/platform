use anyhow::Result;
use common::state::AppState;
use dotenvy::dotenv;

mod consumer;
mod jobs;
pub mod metrics;
mod scheduler;
mod tasks;
mod throttler;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    common::init_telemetry("platform-worker")
        .map_err(|e| anyhow::anyhow!("failed to initialize telemetry: {e}"))?;

    let app_state = AppState::new_from_env().await.unwrap();

    agent_engine::init_shared_sandbox_runtime(app_state.nats_client.clone())
        .await
        .map_err(|e| anyhow::anyhow!("init sandbox runtime: {e}"))?;

    let scheduler = scheduler::JobScheduler::new(app_state.clone());
    scheduler.start().await?;

    let consumer = consumer::NatsConsumer::new(app_state.clone()).await?;
    consumer.start_consuming().await?;

    Ok(())
}
