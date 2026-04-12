use axum::{
    Router,
    routing::{get, post},
};
use common::state::AppState;
use dotenvy::dotenv;
use gateway::handlers::{check_authz, health};
use gateway::{GatewayState, RateLimiter};
use platform::http_tracing::apply_http_trace_layer;
use std::net::SocketAddr;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dotenv().ok();
    common::init_telemetry("gateway-api")?;

    let app_state = AppState::new_from_env()
        .await
        .expect("Failed to initialize AppState");

    let rate_limiter = RateLimiter::new(app_state.clone()).await?;

    let gateway_state = GatewayState {
        rate_limiter,
        app_state,
    };

    let app = apply_http_trace_layer(
        Router::new()
            .route("/v1/authz/check", post(check_authz))
            .route("/health", get(health))
            .with_state(gateway_state),
    );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3002").await?;

    info!("Gateway listening on 0.0.0.0:3002");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
