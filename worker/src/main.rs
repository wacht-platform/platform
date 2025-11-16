use anyhow::Result;
use common::state::AppState;
use dotenvy::dotenv;
use tracing::Level;
use tracing_subscriber;

mod consumer;
mod jobs;
mod scheduler;
mod tasks;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let app_state = AppState::new_from_env().await.unwrap();

    // Start job scheduler
    let scheduler = scheduler::JobScheduler::new(app_state.clone());
    scheduler.start().await?;

    // Start NATS consumer
    let consumer = consumer::NatsConsumer::new(app_state).await?;
    consumer.start_consuming().await?;

    Ok(())
}
