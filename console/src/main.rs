mod api;
mod application;
pub use shared as core;

use dotenvy::dotenv;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let _ = rustls::crypto::ring::default_provider().install_default();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app_state = shared::state::AppState::new_from_env().await?;

    let app = application::new(app_state);

    let port = "0.0.0.0:3001";

    let listener = tokio::net::TcpListener::bind(port).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
