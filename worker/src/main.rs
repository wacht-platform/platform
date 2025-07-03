use anyhow::{Result, anyhow};
use dotenvy::dotenv;
use shared::state::AppState;
use std::env::var as env;
use tracing::Level;
use tracing_subscriber;

mod nats_consumer;
mod nats_types;
mod tasks;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let app_state = AppState::new_from_env().await.unwrap();
    let consumer = nats_consumer::NatsConsumer::new(app_state).await?;
    consumer.start_consuming().await?;

    Ok(())
}
