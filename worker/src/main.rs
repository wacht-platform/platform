use anyhow::Result;
use dotenvy::dotenv;
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

    let consumer = nats_consumer::NatsConsumer::new(&env("NATS_URL").unwrap()).await?;
    consumer.start_consuming().await?;

    Ok(())
}
