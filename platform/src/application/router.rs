use axum::{
    Router,
    body::Body,
    http::{HeaderValue, Request, StatusCode, header::CONTENT_TYPE, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::api;
use crate::application::response::ApiErrorResponse;
use crate::http_tracing::apply_http_trace_layer;
use common::state::AppState;

mod ai_routes;
mod api_auth_routes;
mod backend_router;
mod console_router;
mod deployment_routes;
mod frontend_router;
mod oauth_router;
mod server_routes;

pub use backend_router::create_backend_router;
pub use console_router::create_console_router;
pub use frontend_router::create_frontend_router;
pub use oauth_router::create_oauth_router;

fn cors_layer() -> CorsLayer {
    let allow_origin = AllowOrigin::predicate(|origin: &HeaderValue, _req: &Parts| {
        origin.to_str().ok().is_some_and(is_allowed_origin)
    });

    CorsLayer::new()
        .allow_origin(allow_origin)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any)
}

fn is_allowed_origin(origin: &str) -> bool {
    if origin == "https://console.wacht.dev" {
        return true;
    }

    let local_prefixes = [
        "http://localhost",
        "https://localhost",
        "http://127.0.0.1",
        "https://127.0.0.1",
    ];
    local_prefixes
        .iter()
        .any(|prefix| origin == *prefix || origin.starts_with(&format!("{prefix}:")))
}

fn default_error_message_for_status(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "Bad request",
        StatusCode::UNAUTHORIZED => "Unauthorized",
        StatusCode::FORBIDDEN => "Forbidden",
        StatusCode::NOT_FOUND => "Not found",
        StatusCode::METHOD_NOT_ALLOWED => "Method not allowed",
        StatusCode::UNPROCESSABLE_ENTITY => "Invalid request payload",
        StatusCode::TOO_MANY_REQUESTS => "Rate limit exceeded",
        _ if status.is_server_error() => "Something went wrong",
        _ => "Request failed",
    }
}

async fn normalize_error_responses(req: Request<Body>, next: Next) -> Response {
    let response = next.run(req).await;
    let status = response.status();

    if status.is_success() {
        return response;
    }

    let content_type_is_json = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("application/json"));

    if content_type_is_json {
        return response;
    }

    ApiErrorResponse::from((status, default_error_message_for_status(status))).into_response()
}

pub(super) fn apply_common_http_layers<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    apply_http_trace_layer(router)
        .layer(axum::middleware::from_fn(normalize_error_responses))
        .layer(cors_layer())
}

pub(super) fn health_routes() -> Router<AppState> {
    Router::new().route("/health", get(api::health::check))
}

pub(super) fn public_webhook_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/public/webhooks/dodo",
            post(api::billing_webhook::handle_dodo_webhook),
        )
        .route(
            "/public/webhooks/prelude/{deployment_id}",
            post(api::prelude_webhook::handle_prelude_webhook),
        )
}

pub(super) fn project_routes() -> Router<AppState> {
    Router::new()
        .route("/projects", get(api::project::get_projects))
        .route("/project", post(api::project::create_project))
        .route(
            "/project/{project_id}/staging-deployment",
            post(api::project::create_staging_deployment),
        )
        .route(
            "/project/{project_id}/production-deployment",
            post(api::project::create_production_deployment),
        )
}

pub(super) fn ai_routes() -> Router<AppState> {
    ai_routes::ai_routes()
}

pub(super) fn base_deployment_routes() -> Router<AppState> {
    deployment_routes::base_deployment_routes()
}

pub(super) fn billing_routes() -> Router<AppState> {
    server_routes::billing_routes()
}

pub(super) fn console_specific_routes() -> Router<AppState> {
    server_routes::console_specific_routes()
}

pub(super) fn backend_specific_routes() -> Router<AppState> {
    server_routes::backend_specific_routes()
}
