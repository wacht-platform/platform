use axum::{
    Json,
    extract::{Path, Query, State},
};
use common::state::AppState;
use dto::json::webhook_requests::{
    CreateWebhookEndpointRequest, ListWebhookEndpointsQuery, ReactivateEndpointResponse,
    TestWebhookEndpointRequest, TestWebhookEndpointResponse, UpdateWebhookEndpointRequest,
    WebhookEndpoint as WebhookEndpointDto,
};
use models::webhook::WebhookEndpoint;

use crate::application::{
    response::{ApiResult, PaginatedResponse},
    webhook_endpoints as webhook_endpoints_app,
};
use crate::middleware::RequireDeployment;

pub async fn list_webhook_endpoints(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<ListWebhookEndpointsQuery>,
) -> ApiResult<PaginatedResponse<WebhookEndpointDto>> {
    let endpoints =
        webhook_endpoints_app::list_webhook_endpoints(&app_state, deployment_id, app_slug, params)
            .await?;

    Ok(endpoints.into())
}

pub async fn create_webhook_endpoint(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateWebhookEndpointRequest>,
) -> ApiResult<WebhookEndpoint> {
    let endpoint =
        webhook_endpoints_app::create_webhook_endpoint(&app_state, deployment_id, request).await?;

    Ok(endpoint.into())
}

pub async fn create_webhook_endpoint_for_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Json(mut request): Json<CreateWebhookEndpointRequest>,
) -> ApiResult<WebhookEndpoint> {
    request.app_slug = app_slug;
    create_webhook_endpoint(
        State(app_state),
        RequireDeployment(deployment_id),
        Json(request),
    )
    .await
}

pub async fn update_webhook_endpoint(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(endpoint_id): Path<i64>,
    Json(request): Json<UpdateWebhookEndpointRequest>,
) -> ApiResult<WebhookEndpoint> {
    let endpoint = webhook_endpoints_app::update_webhook_endpoint(
        &app_state,
        deployment_id,
        endpoint_id,
        request,
    )
    .await?;
    Ok(endpoint.into())
}

pub async fn update_webhook_endpoint_for_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, endpoint_id)): Path<(String, i64)>,
    Json(request): Json<UpdateWebhookEndpointRequest>,
) -> ApiResult<WebhookEndpoint> {
    webhook_endpoints_app::ensure_endpoint_belongs_to_app(
        &app_state,
        deployment_id,
        app_slug,
        endpoint_id,
    )
    .await
    .map_err(webhook_endpoints_app::map_error_to_api)?;

    update_webhook_endpoint(
        State(app_state),
        RequireDeployment(deployment_id),
        Path(endpoint_id),
        Json(request),
    )
    .await
}

pub async fn delete_webhook_endpoint(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(endpoint_id): Path<i64>,
) -> ApiResult<()> {
    webhook_endpoints_app::delete_webhook_endpoint(&app_state, deployment_id, endpoint_id).await?;

    Ok(().into())
}

pub async fn delete_webhook_endpoint_for_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, endpoint_id)): Path<(String, i64)>,
) -> ApiResult<()> {
    webhook_endpoints_app::ensure_endpoint_belongs_to_app(
        &app_state,
        deployment_id,
        app_slug,
        endpoint_id,
    )
    .await
    .map_err(webhook_endpoints_app::map_error_to_api)?;

    delete_webhook_endpoint(
        State(app_state),
        RequireDeployment(deployment_id),
        Path(endpoint_id),
    )
    .await
}

pub async fn reactivate_webhook_endpoint(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(endpoint_id): Path<i64>,
) -> ApiResult<ReactivateEndpointResponse> {
    let response =
        webhook_endpoints_app::reactivate_webhook_endpoint(&app_state, deployment_id, endpoint_id)
            .await?;

    Ok(response.into())
}

pub async fn test_webhook_endpoint(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((_app_name, endpoint_id)): Path<(String, i64)>,
    Json(request): Json<TestWebhookEndpointRequest>,
) -> ApiResult<TestWebhookEndpointResponse> {
    let response = webhook_endpoints_app::test_webhook_endpoint(
        &app_state,
        deployment_id,
        endpoint_id,
        request,
    )
    .await?;

    Ok(response.into())
}
