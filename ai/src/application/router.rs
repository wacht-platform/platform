use crate::api::ws;
use crate::application::HttpState;
use axum::Router;
use axum::routing::get;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

fn router() -> Router<HttpState> {
    Router::new().route("/", get(ws::handler))
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
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
}
