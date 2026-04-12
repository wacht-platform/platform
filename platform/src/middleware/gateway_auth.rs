use super::deployment_context::backend_deployment_middleware;
use crate::application::response::ApiErrorResponse;
use axum::{
    body::Body,
    extract::{Request, State},
    response::Response,
};
use common::state::AppState;

pub async fn gateway_auth_middleware(
    State(state): State<AppState>,
    req: Request<Body>,
    next: axum::middleware::Next,
) -> Result<Response, ApiErrorResponse> {
    backend_deployment_middleware(State(state), req, next).await
}
