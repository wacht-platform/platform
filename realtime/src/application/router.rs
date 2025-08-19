use crate::api::{agent, notifications};
use crate::application::HttpState;
use crate::middleware::HostExtractorMiddleware;
use axum::{middleware, Router};
use axum::routing::get;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

fn router() -> Router<HttpState> {
    Router::new()
        .route("/agent", get(agent::agent_stream_handler))
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

pub fn create_router(state: HttpState) -> Router {
    let cors = configure_cors();
    let router = router();

    router
        .layer(middleware::from_fn(HostExtractorMiddleware::extract_host))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
}
