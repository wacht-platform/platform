use crate::realtime::api::notifications;
use crate::realtime::middleware::HostExtractorMiddleware;
use crate::http_tracing::apply_http_trace_layer;
use axum::routing::get;
use axum::{Router, middleware};
use common::state::AppState;
use tower_http::cors::{Any, CorsLayer};

fn router() -> Router<AppState> {
    Router::new().route(
        "/notifications",
        get(notifications::notification_stream_handler),
    )
}

fn configure_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}

pub fn create_router(state: AppState) -> Router {
    let cors = configure_cors();
    let router = router();

    apply_http_trace_layer(
        router
            .layer(middleware::from_fn(HostExtractorMiddleware::extract_host))
            .with_state(state),
    )
    .layer(cors)
}
