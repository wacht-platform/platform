use common::state::AppState;
use dotenvy::dotenv;
use platform::application;
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

    let app_state = AppState::new_from_env().await?;
    let app = application::backend_router(app_state).await;

    let port = std::env::var("BACKEND_API_PORT").unwrap_or_else(|_| "3001".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;

    tracing::info!("Backend API listening on port {}", port);
    axum::serve(listener, app).await?;

    Ok(())
}
