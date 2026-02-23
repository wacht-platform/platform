use common::state::AppState;
use dotenvy::dotenv;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let _ = rustls::crypto::ring::default_provider().install_default();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("error"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app = platform::realtime::router(AppState::new_from_env().await?);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3002").await?;
    axum::serve(listener, app).await?;

    Ok(())
}
