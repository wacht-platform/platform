use axum::extract::{Json, Path, Query, State};
use queries::webhook_analytics::{
    GetWebhookAnalyticsQuery, GetWebhookTimeseriesQuery,
};
use models::webhook_analytics::{
    WebhookAnalyticsResult, WebhookTimeseriesResult,
};

use crate::application::{HttpState, response::ApiResult};
use crate::middleware::RequireDeployment;
use commands::{
    Command,
    webhook_app::{
        CreateWebhookAppCommand, DeleteWebhookAppCommand, RotateWebhookSecretCommand,
        UpdateWebhookAppCommand,
    },
    webhook_endpoint::{
        CreateWebhookEndpointCommand, DeleteWebhookEndpointCommand,
        UpdateWebhookEndpointCommand,
    },
    webhook_trigger::{
        BatchTriggerWebhookEventsCommand, ReplayWebhookDeliveryCommand, TriggerWebhookEventCommand,
    },
};
use dto::json::webhook_requests::{*, WebhookEndpoint as WebhookEndpointDTO};
use dto::clickhouse::webhook::WebhookDelivery;
use models::webhook::{WebhookApp, WebhookEndpoint, WebhookEventTrigger};
use queries::{
    Query as QueryTrait,
    webhook::{
        GetWebhookAppsQuery, GetWebhookDeliveryStatusQuery,
        GetWebhookEndpointsQuery, GetWebhookEndpointsWithSubscriptionsQuery, GetWebhookEventsQuery,
    },
};

pub async fn list_webhook_apps(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ListWebhookAppsQuery>,
) -> ApiResult<ListWebhookAppsResponse> {
    let include_inactive = params.include_inactive.unwrap_or(false);

    let apps = GetWebhookAppsQuery::new(deployment_id)
        .with_inactive(include_inactive)
        .execute(&app_state)
        .await?;

    Ok(ListWebhookAppsResponse {
        total: apps.len(),
        apps,
    }
    .into())
}

pub async fn create_webhook_app(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateWebhookAppRequest>,
) -> ApiResult<WebhookApp> {
    let mut command = CreateWebhookAppCommand::new(deployment_id, request.name);

    if let Some(description) = request.description {
        command = command.with_description(description);
    }

    if !request.events.is_empty() {
        command = command.with_events(request.events);
    }

    let app = command.execute(&app_state).await?;

    Ok(app.into())
}

pub async fn update_webhook_app(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
    Json(request): Json<UpdateWebhookAppRequest>,
) -> ApiResult<WebhookApp> {
    let command = UpdateWebhookAppCommand {
        deployment_id,
        app_name,
        new_name: request.name,
        description: request.description,
        is_active: request.is_active,
    };

    let app = command.execute(&app_state).await?;
    Ok(app.into())
}

pub async fn delete_webhook_app(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
) -> ApiResult<()> {
    let command = DeleteWebhookAppCommand {
        deployment_id,
        app_name,
    };
    command.execute(&app_state).await?;

    Ok(().into())
}

pub async fn rotate_webhook_secret(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
) -> ApiResult<WebhookApp> {
    let command = RotateWebhookSecretCommand {
        deployment_id,
        app_name,
    };
    let app = command.execute(&app_state).await?;

    Ok(app.into())
}

pub async fn get_webhook_events(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
) -> ApiResult<GetAvailableEventsResponse> {
    let events = GetWebhookEventsQuery::new(deployment_id, app_name)
        .execute(&app_state)
        .await?;

    Ok(GetAvailableEventsResponse { events }.into())
}

pub async fn list_webhook_endpoints(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ListWebhookEndpointsQuery>,
) -> ApiResult<ListWebhookEndpointsResponse> {
    let include_inactive = params.include_inactive.unwrap_or(false);

    let mut query = GetWebhookEndpointsWithSubscriptionsQuery::new(deployment_id)
        .with_inactive(include_inactive);

    if let Some(app_name) = params.app_name {
        query = query.for_app(app_name);
    }

    let endpoints = query.execute(&app_state).await?;

    Ok(ListWebhookEndpointsResponse {
        total: endpoints.len(),
        endpoints,
    }
    .into())
}

pub async fn create_webhook_endpoint(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateWebhookEndpointRequest>,
) -> ApiResult<WebhookEndpoint> {
    use commands::webhook_endpoint::EventSubscriptionData;

    // Convert API subscriptions to command subscriptions
    let subscriptions: Vec<EventSubscriptionData> = request
        .subscriptions
        .into_iter()
        .map(|sub| EventSubscriptionData {
            event_name: sub.event_name,
            filter_rules: sub.filter_rules,
        })
        .collect();

    let command = CreateWebhookEndpointCommand {
        deployment_id,
        app_name: request.app_name,
        url: request.url,
        description: request.description,
        headers: request.headers,
        subscriptions,
        max_retries: request.max_retries,
        timeout_seconds: request.timeout_seconds,
    };

    let endpoint = command.execute(&app_state).await?;

    Ok(endpoint.into())
}

pub async fn update_webhook_endpoint(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(endpoint_id): Path<i64>,
    Json(request): Json<UpdateWebhookEndpointRequest>,
) -> ApiResult<WebhookEndpoint> {
    let command = UpdateWebhookEndpointCommand {
        endpoint_id,
        deployment_id,
        url: request.url,
        description: request.description,
        headers: request.headers,
        max_retries: request.max_retries,
        timeout_seconds: request.timeout_seconds,
        is_active: request.is_active,
    };

    let endpoint = command.execute(&app_state).await?;
    Ok(endpoint.into())
}

pub async fn delete_webhook_endpoint(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(endpoint_id): Path<i64>,
) -> ApiResult<()> {
    let command = DeleteWebhookEndpointCommand {
        endpoint_id,
        deployment_id,
    };
    command.execute(&app_state).await?;

    Ok(().into())
}

pub async fn trigger_webhook_event(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<TriggerWebhookEventRequest>,
) -> ApiResult<TriggerWebhookEventResponse> {
    let mut command = TriggerWebhookEventCommand::new(
        deployment_id,
        request.app_name,
        request.event_name,
        request.payload,
    );

    if let Some(context) = request.filter_context {
        command = command.with_filter_context(context);
    }

    let result = command.execute(&app_state).await?;

    Ok(TriggerWebhookEventResponse {
        delivery_ids: result.delivery_ids,
        filtered_count: result.filtered_count,
        delivered_count: result.delivered_count,
    }
    .into())
}

pub async fn batch_trigger_webhook_events(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<BatchTriggerWebhookEventsRequest>,
) -> ApiResult<Vec<TriggerWebhookEventResponse>> {
    let results = BatchTriggerWebhookEventsCommand {
        deployment_id,
        app_name: request.app_name,
        events: request
            .events
            .into_iter()
            .map(|e| WebhookEventTrigger {
                event_name: e.event_name,
                payload: e.payload,
                filter_context: e.filter_context,
            })
            .collect(),
    }
    .execute(&app_state)
    .await?;

    let response: Vec<TriggerWebhookEventResponse> = results
        .into_iter()
        .map(|r| TriggerWebhookEventResponse {
            delivery_ids: r.delivery_ids,
            filtered_count: r.filtered_count,
            delivered_count: r.delivered_count,
        })
        .collect();

    Ok(response.into())
}

pub async fn replay_webhook_delivery(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<ReplayWebhookDeliveryRequest>,
) -> ApiResult<ReplayWebhookDeliveryResponse> {
    let new_delivery_id = ReplayWebhookDeliveryCommand {
        delivery_id: request.delivery_id,
        deployment_id,
    }
    .execute(&app_state)
    .await?;

    Ok(ReplayWebhookDeliveryResponse { new_delivery_id }.into())
}

pub async fn get_webhook_deliveries(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<dto::json::webhook_requests::GetWebhookDeliveriesQuery>,
) -> ApiResult<dto::json::webhook_requests::GetWebhookDeliveriesResponse> {
    let limit = params.limit.unwrap_or(100) as usize;
    let offset = params.offset.unwrap_or(0) as usize;
    
    let deliveries = app_state
        .clickhouse_service
        .get_webhook_deliveries(
            deployment_id,
            params.app_name,
            params.status.as_deref(),
            params.event_name.as_deref(),
            limit,
            offset,
        )
        .await?;

    // Convert Vec<serde_json::Value> to Vec<WebhookDelivery>
    let webhook_deliveries: Vec<dto::clickhouse::webhook::WebhookDelivery> = deliveries
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect();

    let total = webhook_deliveries.len();

    Ok(dto::json::webhook_requests::GetWebhookDeliveriesResponse {
        deliveries: webhook_deliveries,
        total,
    }
    .into())
}

pub async fn get_webhook_delivery_details(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(delivery_id): Path<String>,
) -> ApiResult<dto::json::webhook_requests::WebhookDeliveryDetails> {
    let delivery_id = delivery_id.parse::<i64>().map_err(|_| {
        (axum::http::StatusCode::BAD_REQUEST, "Invalid delivery ID")
    })?;

    let details = app_state
        .clickhouse_service
        .get_webhook_delivery_details(deployment_id, delivery_id)
        .await?;

    // Convert serde_json::Value to WebhookDeliveryDetails
    let delivery_details: dto::json::webhook_requests::WebhookDeliveryDetails = 
        serde_json::from_value(details)
            .map_err(|e| {
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to parse delivery details: {}", e),
                )
            })?;

    Ok(delivery_details.into())
}

pub async fn retry_webhook_delivery(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(delivery_id): Path<String>,
) -> ApiResult<dto::json::webhook_requests::RetryWebhookDeliveryResponse> {
    use dto::json::nats::NatsTaskMessage;

    let delivery_id = delivery_id.parse::<i64>().map_err(|_| {
        (axum::http::StatusCode::BAD_REQUEST, "Invalid delivery ID")
    })?;

    // Queue retry task for background processing
    let task_message = NatsTaskMessage {
        task_type: "webhook.retry".to_string(),
        task_id: format!("webhook-retry-{}-{}", delivery_id, deployment_id),
        payload: serde_json::json!({
            "delivery_id": delivery_id,
            "deployment_id": deployment_id
        }),
    };

    app_state
        .nats_client
        .publish(
            "worker.tasks.webhook.retry",
            serde_json::to_vec(&task_message)
                .map_err(|e| {
                    (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to serialize task: {}", e),
                    )
                })?
                .into(),
        )
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to queue retry task: {}", e),
            )
        })?;
    
    Ok(dto::json::webhook_requests::RetryWebhookDeliveryResponse {
        delivery_id,
        status: "retrying".to_string(),
        message: "Delivery queued for retry".to_string(),
    }
    .into())
}

pub async fn get_webhook_delivery_status(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<GetWebhookDeliveryStatusRequest>,
) -> ApiResult<WebhookDeliveryStatus> {
    let delivery = GetWebhookDeliveryStatusQuery::new(params.delivery_id, deployment_id)
        .execute(&app_state)
        .await?;

    let status = if delivery.attempts >= delivery.max_attempts {
        "failed".to_string()
    } else if delivery.attempts > 0 {
        "retrying".to_string()
    } else {
        "pending".to_string()
    };

    Ok(WebhookDeliveryStatus {
        id: delivery.id,
        endpoint_id: delivery.endpoint_id,
        event_name: delivery.event_name,
        attempts: delivery.attempts,
        max_attempts: delivery.max_attempts,
        next_retry_at: delivery.next_retry_at,
        created_at: delivery.created_at,
        status,
    }
    .into())
}

pub async fn reactivate_webhook_endpoint(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<ReactivateEndpointRequest>,
) -> ApiResult<ReactivateEndpointResponse> {
    use commands::webhook_endpoint::ReactivateEndpointCommand;

    // Reactivate the endpoint and clear failure counter
    let endpoint = ReactivateEndpointCommand {
        endpoint_id: request.endpoint_id,
        deployment_id,
    }
    .execute(&app_state)
    .await?;

    // Log reactivation to ClickHouse
    let ch_delivery = WebhookDelivery {
        deployment_id,
        delivery_id: app_state.sf.next_id().unwrap() as i64,
        app_name: endpoint.app_name.clone(),
        endpoint_id: endpoint.id,
        endpoint_url: endpoint.url.clone(),
        event_name: "endpoint.reactivated".to_string(),
        status: "reactivated".to_string(),
        http_status_code: None,
        response_time_ms: None,
        attempt_number: 0,
        max_attempts: 1,
        error_message: None,
        filtered_reason: None,
        payload_s3_key: "endpoint-reactivation".to_string(),
        response_body: None,
        response_headers: None,
        timestamp: chrono::Utc::now(),
    };

    let _ = app_state
        .clickhouse_service
        .insert_webhook_delivery(&ch_delivery)
        .await;

    Ok(ReactivateEndpointResponse {
        success: true,
        message: format!("Endpoint {} reactivated successfully", endpoint.url),
    }
    .into())
}

pub async fn test_webhook_endpoint(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<TestWebhookEndpointRequest>,
) -> ApiResult<TestWebhookEndpointResponse> {
    use commands::webhook_endpoint::TestWebhookEndpointCommand;

    let test_payload = serde_json::json!({
        "test": true,
        "event": request.event_name,
        "payload": request.payload,
        "timestamp": chrono::Utc::now()
    });

    let result = TestWebhookEndpointCommand {
        endpoint_id: request.endpoint_id,
        deployment_id,
        test_payload,
    }
    .execute(&app_state)
    .await?;

    Ok(TestWebhookEndpointResponse {
        success: result.success,
        status_code: result.status_code,
        response_time_ms: result.response_time_ms,
        response_body: result.response_body,
        error: result.error,
    }
    .into())
}

pub async fn get_webhook_analytics(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<WebhookAnalyticsQuery>,
) -> ApiResult<WebhookAnalyticsResult> {
    let mut query = GetWebhookAnalyticsQuery::new(deployment_id);

    if let Some(app_id) = params.app_id {
        query = query.with_app_name(app_id.to_string());
    }

    if let Some(endpoint_id) = params.endpoint_id {
        query = query.with_endpoint(endpoint_id);
    }

    if let (Some(start), Some(end)) = (params.start_date, params.end_date) {
        query = query.with_date_range(start, end);
    }

    let result = query.execute(&app_state).await?;

    Ok(result.into())
}

pub async fn get_webhook_timeseries(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<WebhookTimeseriesQuery>,
) -> ApiResult<WebhookTimeseriesResult> {
    let mut query = GetWebhookTimeseriesQuery::new(deployment_id, params.interval);

    if let Some(app_id) = params.app_id {
        query = query.with_app_name(app_id.to_string());
    }

    if let Some(endpoint_id) = params.endpoint_id {
        query = query.with_endpoint(endpoint_id);
    }

    if let (Some(start), Some(end)) = (params.start_date, params.end_date) {
        query = query.with_date_range(start, end);
    }

    let result = query.execute(&app_state).await?;

    Ok(result.into())
}
