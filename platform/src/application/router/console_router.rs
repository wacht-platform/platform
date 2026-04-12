use axum::Router;
use common::state::AppState;

use crate::middleware::deployment_access::deployment_access_middleware;
use crate::middleware::platform_source::mark_console_platform_source;

pub async fn create_console_router(state: AppState) -> Router {
    wacht::init_from_env().await.unwrap();

    use wacht::middleware::AuthLayer;
    let auth_layer = AuthLayer::new();

    let deployment_routes = super::base_deployment_routes()
        .merge(super::ai_routes())
        .merge(super::console_specific_routes())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            deployment_access_middleware,
        ));

    let protected_routes = Router::new()
        .merge(super::project_routes())
        .merge(super::billing_routes())
        .nest("/deployments/{deployment_id}", deployment_routes)
        .layer(auth_layer);

    super::apply_common_http_layers(
        Router::new()
            .merge(super::health_routes())
            .merge(super::public_webhook_routes())
            .merge(protected_routes)
            .with_state(state)
            .layer(axum::middleware::from_fn(mark_console_platform_source)),
    )
}
