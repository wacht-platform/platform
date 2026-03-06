use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use common::state::AppState;
use dto::{
    clickhouse::webhook::WebhookDeliveryListResponse,
    json::webhook_requests::{GetAppWebhookDeliveriesQuery, WebhookDeliveryDetails},
};
use models::webhook_analytics::WebhookAnalyticsResult;

use crate::application::{
    response::{ApiResult, PaginatedResponse},
    webhook_deliveries as webhook_deliveries_use_cases,
};
use crate::middleware::RequireDeployment;

pub async fn get_webhook_delivery_details(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(delivery_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<WebhookDeliveryDetails> {
    let delivery = webhook_deliveries_use_cases::get_webhook_delivery_details(
        &app_state,
        deployment_id,
        delivery_id,
        params,
    )
    .await
    .map_err(webhook_deliveries_use_cases::map_error_to_api)?;

    Ok(delivery.into())
}

pub async fn get_webhook_delivery_details_for_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, delivery_id)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<WebhookDeliveryDetails> {
    let delivery = webhook_deliveries_use_cases::get_webhook_delivery_details_for_app(
        &app_state,
        deployment_id,
        app_slug,
        delivery_id,
        params,
    )
    .await
    .map_err(webhook_deliveries_use_cases::map_error_to_api)?;

    Ok(delivery.into())
}

pub async fn get_webhook_stats(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
) -> ApiResult<WebhookAnalyticsResult> {
    let stats =
        webhook_deliveries_use_cases::get_webhook_stats(&app_state, deployment_id, app_slug).await?;

    Ok(stats.into())
}

pub async fn get_app_webhook_deliveries(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<GetAppWebhookDeliveriesQuery>,
) -> ApiResult<PaginatedResponse<WebhookDeliveryListResponse>> {
    let deliveries = webhook_deliveries_use_cases::get_app_webhook_deliveries(
        &app_state,
        deployment_id,
        app_slug,
        params,
    )
    .await?;

    Ok(deliveries.into())
}
