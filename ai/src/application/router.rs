use crate::api::ws;
use axum::Router;
use axum::routing::get;
use shared::state::AppState;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

fn router() -> Router<AppState> {
    Router::new().route("/", get(ws::handler))
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
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
}
