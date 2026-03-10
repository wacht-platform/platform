use common::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    platform::bootstrap::init_runtime_default_env_with_rustls();

    let app = platform::realtime::router(AppState::new_from_env().await?);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3002").await?;
    axum::serve(listener, app).await?;

    Ok(())
}
