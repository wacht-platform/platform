use anyhow::Result;
use common::state::AppState;
use dotenvy::dotenv;
use tracing::Level;
use tracing_subscriber;

mod consumer;
mod tasks;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let app_state = AppState::new_from_env().await.unwrap();
    let consumer = consumer::NatsConsumer::new(app_state).await?;
    consumer.start_consuming().await?;

    Ok(())
}
