use common::state::AppState;
use platform::application;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    platform::bootstrap::init_runtime_override_env_with_rustls();

    let app_state = AppState::new_from_env().await?;
    let app = application::console_router(app_state).await;

    let port = std::env::var("CONSOLE_API_PORT").unwrap_or_else(|_| "3001".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;

    tracing::info!("Console API listening on port {}", port);
    axum::serve(listener, app).await?;

    Ok(())
}
