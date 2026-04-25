use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use common::state::AppState;
use dto::{
    clickhouse::webhook::WebhookDeliveryListResponse,
    json::{
        WebhookStats,
        webhook_requests::{GetAppWebhookDeliveriesQuery, WebhookDeliveryDetails},
    },
};

use crate::application::{
    response::{ApiResult, PaginatedResponse},
    webhook_deliveries as webhook_deliveries_app,
};
use crate::middleware::{AppSlugParams, DeliveryIdParams, RequireDeployment};

pub async fn get_webhook_delivery_details(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(DeliveryIdParams { delivery_id, .. }): Path<DeliveryIdParams>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<WebhookDeliveryDetails> {
    let delivery = webhook_deliveries_app::get_webhook_delivery_details(
        &app_state,
        deployment_id,
        delivery_id,
        params,
    )
    .await
    .map_err(webhook_deliveries_app::map_error_to_api)?;

    Ok(delivery.into())
}

pub async fn get_webhook_delivery_details_for_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, delivery_id)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<WebhookDeliveryDetails> {
    let delivery = webhook_deliveries_app::get_webhook_delivery_details_for_app(
        &app_state,
        deployment_id,
        app_slug,
        delivery_id,
        params,
    )
    .await
    .map_err(webhook_deliveries_app::map_error_to_api)?;

    Ok(delivery.into())
}

pub async fn get_webhook_stats(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(AppSlugParams { app_slug, .. }): Path<AppSlugParams>,
) -> ApiResult<WebhookStats> {
    let stats =
        webhook_deliveries_app::get_webhook_stats(&app_state, deployment_id, app_slug).await?;

    Ok(stats.into())
}

pub async fn get_app_webhook_deliveries(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(AppSlugParams { app_slug, .. }): Path<AppSlugParams>,
    Query(params): Query<GetAppWebhookDeliveriesQuery>,
) -> ApiResult<PaginatedResponse<WebhookDeliveryListResponse>> {
    let deliveries = webhook_deliveries_app::get_app_webhook_deliveries(
        &app_state,
        deployment_id,
        app_slug,
        params,
    )
    .await?;

    Ok(deliveries.into())
}
