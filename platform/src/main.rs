mod api;
mod application;
mod middleware;

use common::state::AppState;
use dotenvy::dotenv;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let _ = rustls::crypto::ring::default_provider().install_default();

    let app = application::new(AppState::new_from_env().await?).await;
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await?;
    axum::serve(listener, app).await?;

    Ok(())
}
