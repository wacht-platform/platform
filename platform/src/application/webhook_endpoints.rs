use axum::http::StatusCode;
use commands::{
    webhook_endpoint::{
        CreateWebhookEndpointCommand, DeleteWebhookEndpointCommand, EventSubscriptionData,
        ReactivateEndpointCommand, TestWebhookEndpointCommand, UpdateWebhookEndpointCommand,
    },
};
use common::db_router::ReadConsistency;
use common::error::AppError;
use common::state::AppState;
use dto::{
    clickhouse::webhook::WebhookLog,
    json::webhook_requests::{
        CreateWebhookEndpointRequest, ListWebhookEndpointsQuery, ReactivateEndpointResponse,
        TestWebhookEndpointRequest, TestWebhookEndpointResponse, UpdateWebhookEndpointRequest,
        WebhookEndpoint as WebhookEndpointDto,
    },
};
use models::webhook::WebhookEndpoint;
use queries::{
    webhook::{GetWebhookEndpointsQuery, GetWebhookEndpointsWithSubscriptionsQuery},
};

use crate::{api::pagination::paginate_results, application::response::PaginatedResponse};

fn serialize_optional_json<T: serde::Serialize>(
    value: Option<T>,
    field_name: &'static str,
) -> Result<Option<serde_json::Value>, AppError> {
    value
        .map(serde_json::to_value)
        .transpose()
        .map_err(|e| AppError::Validation(format!("Invalid {}: {}", field_name, e)))
}

fn map_event_subscriptions(
    subscriptions: Vec<dto::json::webhook_requests::EventSubscription>,
) -> Vec<EventSubscriptionData> {
    subscriptions
        .into_iter()
        .map(|subscription| EventSubscriptionData {
            event_name: subscription.event_name,
            filter_rules: subscription.filter_rules,
        })
        .collect()
}

pub async fn ensure_endpoint_belongs_to_app(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
    endpoint_id: i64,
) -> Result<(), AppError> {
    let endpoints = GetWebhookEndpointsQuery::new(deployment_id)
        .for_app(app_slug)
        .with_inactive(true)
        .execute_with(app_state.db_router.reader(ReadConsistency::Strong))
        .await?;
    if endpoints.iter().any(|endpoint| endpoint.id == endpoint_id) {
        Ok(())
    } else {
        Err(AppError::NotFound("Webhook endpoint not found".to_string()))
    }
}

pub async fn list_webhook_endpoints(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
    params: ListWebhookEndpointsQuery,
) -> Result<PaginatedResponse<WebhookEndpointDto>, AppError> {
    let include_inactive = params.include_inactive.unwrap_or(false);
    let limit = params.limit.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);

    let endpoints = GetWebhookEndpointsWithSubscriptionsQuery::new(deployment_id)
        .with_inactive(include_inactive)
        .for_app(app_slug)
        .with_pagination(Some(limit + 1), Some(offset))
        .execute_with(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    Ok(paginate_results(endpoints, limit, Some(offset as i64)))
}

pub async fn create_webhook_endpoint(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateWebhookEndpointRequest,
) -> Result<WebhookEndpoint, AppError> {
    let rate_limit_config = serialize_optional_json(request.rate_limit_config, "rate_limit_config")?;
    let subscriptions = map_event_subscriptions(request.subscriptions);

    let command = CreateWebhookEndpointCommand::new(deployment_id, request.app_slug, request.url)
        .with_description(request.description)
        .with_headers(request.headers)
        .with_subscriptions(subscriptions)
        .with_max_retries(request.max_retries)
        .with_timeout_seconds(request.timeout_seconds)
        .with_rate_limit_config(rate_limit_config);

    command
        .execute_with(
            app_state.db_router.writer(),
            app_state,
            app_state.sf.next_id()? as i64,
        )
        .await
}

pub async fn update_webhook_endpoint(
    app_state: &AppState,
    deployment_id: i64,
    endpoint_id: i64,
    request: UpdateWebhookEndpointRequest,
) -> Result<WebhookEndpoint, AppError> {
    let rate_limit_config = serialize_optional_json(request.rate_limit_config, "rate_limit_config")?;

    let command = UpdateWebhookEndpointCommand::new(endpoint_id, deployment_id)
        .with_url(request.url)
        .with_description(request.description)
        .with_headers(request.headers)
        .with_max_retries(request.max_retries)
        .with_timeout_seconds(request.timeout_seconds)
        .with_is_active(request.is_active)
        .with_subscriptions(request.subscriptions.map(map_event_subscriptions))
        .with_rate_limit_config(rate_limit_config);

    command
        .execute_with(app_state.db_router.writer(), app_state)
        .await
}

pub async fn delete_webhook_endpoint(
    app_state: &AppState,
    deployment_id: i64,
    endpoint_id: i64,
) -> Result<(), AppError> {
    let command = DeleteWebhookEndpointCommand::new(endpoint_id, deployment_id);
    command.execute_with(app_state.db_router.writer()).await?;
    Ok(())
}

pub async fn reactivate_webhook_endpoint(
    app_state: &AppState,
    deployment_id: i64,
    endpoint_id: i64,
) -> Result<ReactivateEndpointResponse, AppError> {
    let endpoint = ReactivateEndpointCommand::new(endpoint_id, deployment_id)
        .execute_with(app_state.db_router.writer(), app_state)
        .await?;

    if let Ok(log_id) = app_state.sf.next_id() {
        let ch_log = WebhookLog {
            deployment_id,
            delivery_id: log_id as i64,
            app_slug: endpoint.app_slug.clone(),
            endpoint_id: endpoint.id,
            event_name: "endpoint.reactivated".to_string(),
            status: "reactivated".to_string(),
            http_status_code: None,
            response_time_ms: None,
            attempt_number: 0,
            max_attempts: 1,
            payload: None,
            payload_size_bytes: 0,
            response_body: None,
            response_headers: None,
            request_headers: None,
            timestamp: chrono::Utc::now(),
        };

        let _ = app_state.clickhouse_service.insert_webhook_log(&ch_log).await;
    } else {
        tracing::warn!("Failed to generate snowflake id for webhook reactivation log");
    }

    Ok(ReactivateEndpointResponse {
        success: true,
        message: format!("Endpoint {} reactivated successfully", endpoint.url),
    })
}

pub async fn test_webhook_endpoint(
    app_state: &AppState,
    deployment_id: i64,
    endpoint_id: i64,
    request: TestWebhookEndpointRequest,
) -> Result<TestWebhookEndpointResponse, AppError> {
    let test_payload = request.payload.unwrap_or_else(|| {
        serde_json::json!({
            "test": true,
            "event": request.event_name,
            "timestamp": chrono::Utc::now()
        })
    });

    let result = TestWebhookEndpointCommand::new(endpoint_id, deployment_id, test_payload)
        .execute_with(
            app_state.db_router.writer(),
            app_state,
            app_state.sf.next_id()? as i64,
        )
        .await?;

    Ok(TestWebhookEndpointResponse {
        success: result.success,
        status_code: result.status_code,
        response_time_ms: result.response_time_ms,
        response_body: result.response_body,
        error: result.error,
    })
}

pub fn map_error_to_api(err: AppError) -> crate::application::response::ApiErrorResponse {
    match err {
        AppError::NotFound(msg) if msg == "Webhook endpoint not found" => {
            (StatusCode::NOT_FOUND, msg).into()
        }
        other => other.into(),
    }
}
