use common::state::AppState;
use platform::application;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    platform::bootstrap::init_runtime_override_env_with_rustls("backend-api");

    let app_state = AppState::new_from_env().await?;
    let app = application::backend_router(app_state).await;

    let port = std::env::var("BACKEND_API_PORT").unwrap_or_else(|_| "3001".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;

    tracing::info!("Backend API listening on port {}", port);
    axum::serve(listener, app).await?;

    Ok(())
}
