use super::*;
use crate::api::pagination::paginate_results;

pub async fn get_webhook_delivery_details(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(delivery_id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> ApiResult<dto::json::webhook_requests::WebhookDeliveryDetails> {
    let delivery_id = delivery_id
        .parse::<i64>()
        .map_err(|_| (axum::http::StatusCode::BAD_REQUEST, "Invalid delivery ID"))?;

    // Check if status=pending to look in PostgreSQL instead of ClickHouse
    let status = params.get("status").map(|s| s.as_str());

    let delivery = if status == Some("pending") {
        // Check PostgreSQL for active/pending deliveries
        queries::webhook::GetPendingWebhookDeliveryQuery::new(deployment_id, delivery_id)
            .execute(&app_state)
            .await?
    } else {
        // Check ClickHouse for completed deliveries
        app_state
            .clickhouse_service
            .get_webhook_delivery_details(deployment_id, delivery_id)
            .await?
    };

    // Parse payload from storage JSON string to structured JSON response
    let payload = delivery
        .payload
        .clone()
        .and_then(|p| serde_json::from_str(&p).ok());

    // Convert WebhookDelivery to WebhookDeliveryDetails
    let delivery_details = dto::json::webhook_requests::WebhookDeliveryDetails {
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
    };

    Ok(delivery_details.into())
}

pub async fn get_webhook_delivery_details_for_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, delivery_id)): Path<(String, String)>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> ApiResult<dto::json::webhook_requests::WebhookDeliveryDetails> {
    GetWebhookAppByNameQuery::new(deployment_id, app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Webhook app not found".to_string()))?;

    get_webhook_delivery_details(
        State(app_state),
        RequireDeployment(deployment_id),
        Path(delivery_id),
        Query(params),
    )
    .await
}

pub async fn get_webhook_stats(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
) -> ApiResult<WebhookAnalyticsResult> {
    let query = GetWebhookAnalyticsQuery::new(deployment_id).with_app_slug(app_slug);

    let result = query.execute(&app_state).await?;

    Ok(result.into())
}

pub async fn get_app_webhook_deliveries(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<GetAppWebhookDeliveriesQuery>,
) -> ApiResult<PaginatedResponse<WebhookDeliveryListResponse>> {
    let limit = params.limit.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);

    // Fetch one extra to determine if there are more
    let delivery_rows = app_state
        .clickhouse_service
        .get_webhook_deliveries(
            deployment_id,
            Some(app_slug.clone()),
            params.status.as_deref(),
            params.event_name.as_deref(),
            (limit + 1) as usize,
            offset as usize,
        )
        .await?;

    let deliveries: Vec<WebhookDeliveryListResponse> =
        delivery_rows.into_iter().map(|row| row.into()).collect();

    Ok(paginate_results(deliveries, limit, Some(offset as i64)).into())
}
