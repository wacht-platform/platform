use crate::realtime::api::{agent_sse, notifications};
use crate::realtime::middleware::HostExtractorMiddleware;
use axum::routing::get;
use axum::{Router, middleware};
use common::state::AppState;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

fn router() -> Router<AppState> {
    Router::new()
        .route("/agent/stream", get(agent_sse::agent_sse_stream_handler))
        .route(
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

    router
        .layer(middleware::from_fn(HostExtractorMiddleware::extract_host))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
}
