use axum::Router;
use common::state::AppState;

use crate::middleware::backend_deployment_middleware;
use crate::middleware::platform_source::mark_backend_platform_source;

pub async fn create_backend_router(state: AppState) -> Router {
    assert!(
        state.wacht_client.is_some(),
        "Backend API requires Wacht gateway client. Ensure WACHT_API_KEY and WACHT_FRONTEND_HOST are set."
    );

    let backend_routes = super::base_deployment_routes()
        .merge(super::ai_routes())
        .merge(super::backend_specific_routes())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            backend_deployment_middleware,
        ));

    super::apply_common_http_layers(
        Router::new()
            .merge(super::health_routes())
            .merge(backend_routes)
            .with_state(state)
            .layer(axum::middleware::from_fn(mark_backend_platform_source)),
    )
}
