use axum::{body::Body, extract::{Request, State}, http::StatusCode, response::Response};
use common::state::AppState;
use super::deployment_context::backend_deployment_middleware;

pub async fn gateway_auth_middleware(
    State(state): State<AppState>,
    req: Request<Body>,
    next: axum::middleware::Next,
) -> Result<Response, (StatusCode, String)> {
    backend_deployment_middleware(State(state), req, next).await
}
