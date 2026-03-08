use std::collections::HashMap;

use axum::http::StatusCode;
use common::db_router::ReadConsistency;
use common::error::AppError;
use common::state::AppState;
use dto::{
    clickhouse::webhook::WebhookDeliveryListResponse,
    json::webhook_requests::{GetAppWebhookDeliveriesQuery, WebhookDeliveryDetails},
};
use models::webhook_analytics::WebhookAnalyticsResult;
use queries::webhook_analytics::GetWebhookAnalyticsQuery;

use crate::{api::pagination::paginate_results, application::response::PaginatedResponse};

async fn ensure_webhook_app_exists(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
) -> Result<(), AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let app = queries::GetWebhookAppByNameQuery::new(deployment_id, app_slug)
        .execute_with_db(reader)
        .await?;
    if app.is_some() {
        Ok(())
    } else {
        Err(AppError::NotFound("Webhook app not found".to_string()))
    }
}

pub async fn get_webhook_delivery_details(
    app_state: &AppState,
    deployment_id: i64,
    delivery_id: String,
    params: HashMap<String, String>,
) -> Result<WebhookDeliveryDetails, AppError> {
    let delivery_id = delivery_id
        .parse::<i64>()
        .map_err(|_| AppError::Validation("Invalid delivery ID".to_string()))?;

    let status = params.get("status").map(|s| s.as_str());

    let delivery = if status == Some("pending") {
        let reader = app_state.db_router.reader(ReadConsistency::Strong);
        queries::webhook::GetPendingWebhookDeliveryQuery::new(deployment_id, delivery_id)
            .execute_with_db(reader)
            .await?
    } else {
        app_state
            .clickhouse_service
            .get_webhook_delivery_details(deployment_id, delivery_id)
            .await?
    };

    let payload = delivery
        .payload
        .clone()
        .and_then(|p| serde_json::from_str(&p).ok());

    Ok(WebhookDeliveryDetails {
        delivery_id: delivery.delivery_id,
        deployment_id: delivery.deployment_id,
        app_slug: delivery.app_slug,
        endpoint_id: delivery.endpoint_id,
        event_name: delivery.event_name,
        status: delivery.status,
        http_status_code: delivery.http_status_code,
        response_time_ms: delivery.response_time_ms,
        attempt_number: delivery.attempt_number,
        max_attempts: delivery.max_attempts,
        payload,
        response_body: delivery.response_body,
        response_headers: delivery
            .response_headers
            .and_then(|h| serde_json::from_str(&h).ok()),
        timestamp: delivery.timestamp,
    })
}

pub async fn get_webhook_delivery_details_for_app(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
    delivery_id: String,
    params: HashMap<String, String>,
) -> Result<WebhookDeliveryDetails, AppError> {
    ensure_webhook_app_exists(app_state, deployment_id, app_slug).await?;
    get_webhook_delivery_details(app_state, deployment_id, delivery_id, params).await
}

pub async fn get_webhook_stats(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
) -> Result<WebhookAnalyticsResult, AppError> {
    let query = GetWebhookAnalyticsQuery::new(deployment_id).with_app_slug(app_slug);
    query.execute_with_deps(app_state).await
}

pub async fn get_app_webhook_deliveries(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
    params: GetAppWebhookDeliveriesQuery,
) -> Result<PaginatedResponse<WebhookDeliveryListResponse>, AppError> {
    let limit = params.limit.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);

    let delivery_rows = app_state
        .clickhouse_service
        .get_webhook_deliveries(
            deployment_id,
            Some(app_slug),
            params.status.as_deref(),
            params.event_name.as_deref(),
            (limit + 1) as usize,
            offset as usize,
        )
        .await?;

    let deliveries: Vec<WebhookDeliveryListResponse> =
        delivery_rows.into_iter().map(|row| row.into()).collect();

    Ok(paginate_results(deliveries, limit, Some(offset as i64)))
}

pub fn map_error_to_api(err: AppError) -> crate::application::response::ApiErrorResponse {
    match err {
        AppError::Validation(msg) if msg == "Invalid delivery ID" => {
            (StatusCode::BAD_REQUEST, msg).into()
        }
        AppError::NotFound(msg) if msg == "Webhook app not found" => {
            (StatusCode::NOT_FOUND, msg).into()
        }
        other => other.into(),
    }
}
