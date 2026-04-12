use axum::Router;
use common::state::AppState;

pub async fn create_frontend_router(state: AppState) -> Router {
    super::apply_common_http_layers(
        Router::new()
            .merge(super::health_routes())
            .with_state(state),
    )
}
