use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;
use chrono::{Datelike, Utc};
use models::webhook_analytics::{WebhookAnalyticsResult, WebhookTimeseriesResult};
use queries::GetWebhookAppByNameQuery;
use queries::webhook_analytics::{GetWebhookAnalyticsQuery, GetWebhookTimeseriesQuery};

use crate::application::response::{ApiResult, PaginatedResponse};
use crate::middleware::RequireDeployment;
use commands::{
    Command,
    webhook_app::{
        CreateWebhookAppCommand, DeleteWebhookAppCommand, RotateWebhookSecretCommand,
        UpdateWebhookAppCommand,
    },
    webhook_endpoint::{
        CreateWebhookEndpointCommand, DeleteWebhookEndpointCommand, UpdateWebhookEndpointCommand,
    },
    webhook_trigger::{BatchTriggerWebhookEventsCommand, TriggerWebhookEventCommand},
};
use common::state::AppState;
use dto::clickhouse::webhook::{WebhookDelivery, WebhookDeliveryListResponse};
use dto::json::webhook_requests::{WebhookEndpoint as WebhookEndpointDto, *};
use models::webhook::{WebhookApp, WebhookEndpoint, WebhookEventTrigger};
use queries::{
    Query as QueryTrait,
    webhook::{
        GetWebhookAppsQuery, GetWebhookEndpointsWithSubscriptionsQuery, GetWebhookEventsQuery,
    },
};

pub async fn list_webhook_apps(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ListWebhookAppsQuery>,
) -> ApiResult<PaginatedResponse<WebhookApp>> {
    let include_inactive = params.include_inactive.unwrap_or(false);
    let limit = params.limit.unwrap_or(50) as u64;
    let offset = params.offset.unwrap_or(0) as u64;

    let mut apps = GetWebhookAppsQuery::new(deployment_id)
        .with_inactive(include_inactive)
        .with_pagination(Some(limit as i64 + 1), Some(offset as i64))
        .execute(&app_state)
        .await?;

    let has_more = apps.len() > limit as usize;
    if has_more {
        apps.pop();
    }

    Ok(PaginatedResponse {
        data: apps,
        has_more,
        limit: Some(limit as i32),
        offset: Some(offset as i32),
    }
    .into())
}

pub async fn create_webhook_app(
    State(app_state): State<AppState>,
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
    State(app_state): State<AppState>,
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

pub async fn get_webhook_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
) -> ApiResult<WebhookApp> {
    let command = GetWebhookAppByNameQuery::new(deployment_id, app_name);

    let app = command
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Webhook app not found".to_string()))?;

    Ok(app.into())
}

pub async fn delete_webhook_app(
    State(app_state): State<AppState>,
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
    State(app_state): State<AppState>,
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
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
) -> ApiResult<GetAvailableEventsResponse> {
    let model_events = GetWebhookEventsQuery::new(deployment_id, app_name)
        .execute(&app_state)
        .await?;

    // Convert model events to SDK format
    let events: Vec<wacht::api::webhooks::WebhookAppEvent> = model_events
        .into_iter()
        .map(|e| wacht::api::webhooks::WebhookAppEvent {
            deployment_id: e.deployment_id.to_string(),
            app_name: e.app_name,
            event_name: e.event_name,
            description: e.description,
            schema: e.schema,
            created_at: e.created_at,
        })
        .collect();

    Ok(GetAvailableEventsResponse { events }.into())
}

pub async fn list_webhook_endpoints(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
    Query(params): Query<ListWebhookEndpointsQuery>,
) -> ApiResult<PaginatedResponse<WebhookEndpointDto>> {
    let include_inactive = params.include_inactive.unwrap_or(false);
    let limit = params.limit.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);

    // Fetch one extra to determine if there are more
    // The query already returns dto::json::webhook_requests::WebhookEndpoint with subscriptions
    let mut endpoints = GetWebhookEndpointsWithSubscriptionsQuery::new(deployment_id)
        .with_inactive(include_inactive)
        .for_app(app_name)
        .with_pagination(Some(limit + 1), Some(offset))
        .execute(&app_state)
        .await?;

    let has_more = endpoints.len() > limit as usize;
    if has_more {
        endpoints.truncate(limit as usize);
    }

    Ok(PaginatedResponse {
        data: endpoints,
        has_more,
        limit: Some(limit),
        offset: Some(offset),
    }
    .into())
}

pub async fn create_webhook_endpoint(
    State(app_state): State<AppState>,
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
    State(app_state): State<AppState>,
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
        subscriptions: request
            .subscriptions
            .map(|subs| subs.into_iter().map(Into::into).collect()),
    };

    let endpoint = command.execute(&app_state).await?;
    Ok(endpoint.into())
}

pub async fn delete_webhook_endpoint(
    State(app_state): State<AppState>,
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
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
    Json(request): Json<TriggerWebhookEventRequest>,
) -> ApiResult<TriggerWebhookEventResponse> {
    let mut command = TriggerWebhookEventCommand::new(
        deployment_id,
        app_name,
        request.event_name,
        request.payload,
    );

    if let Some(context) = request.filter_context {
        command = command.with_filter_context(context);
    }

    let result = command.execute(&app_state).await?;

    tokio::spawn({
        let redis = app_state.redis_client.clone();
        async move {
            if let Ok(mut conn) = redis.get_multiplexed_async_connection().await {
                let now = Utc::now();
                let period = format!("{}-{:02}", now.year(), now.month());
                let prefix = format!("billing:{}:deployment:{}", period, deployment_id);

                let mut pipe = redis::pipe();
                pipe.atomic()
                    .zincr(&format!("{}:metrics", prefix), "webhooks", 1)
                    .ignore()
                    .expire(&format!("{}:metrics", prefix), 5184000)
                    .ignore()
                    .zincr(
                        &format!("billing:{}:dirty_deployments", period),
                        deployment_id,
                        1,
                    )
                    .ignore()
                    .expire(&format!("billing:{}:dirty_deployments", period), 5184000)
                    .ignore();

                let _: Result<(), redis::RedisError> = pipe.query_async(&mut conn).await;
            }
        }
    });

    Ok(TriggerWebhookEventResponse {
        delivery_ids: result.delivery_ids,
        filtered_count: result.filtered_count,
        delivered_count: result.delivered_count,
    }
    .into())
}

pub async fn batch_trigger_webhook_events(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
    Json(request): Json<BatchTriggerWebhookEventsRequest>,
) -> ApiResult<PaginatedResponse<TriggerWebhookEventResponse>> {
    let results = BatchTriggerWebhookEventsCommand {
        deployment_id,
        app_name,
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

    Ok(PaginatedResponse::from(response).into())
}

pub async fn replay_webhook_delivery(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(_app_name): Path<String>,
    Json(request): Json<ReplayWebhookDeliveryRequest>,
) -> ApiResult<ReplayWebhookDeliveryResponse> {
    use dto::json::nats::{NatsTaskMessage, WebhookReplayBatchPayload};

    // Create strongly typed task payload based on request type
    let task_payload = match request {
        ReplayWebhookDeliveryRequest::ByIds {
            delivery_ids,
            include_successful,
        } => WebhookReplayBatchPayload::ByIds {
            deployment_id,
            delivery_ids,
            include_successful,
        },
        ReplayWebhookDeliveryRequest::ByDateRange {
            start_date,
            end_date,
            include_successful,
        } => WebhookReplayBatchPayload::ByDateRange {
            deployment_id,
            start_date,
            end_date,
            include_successful,
        },
    };

    let task_payload_json = serde_json::to_value(task_payload).map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to serialize task payload: {}", e),
        )
    })?;

    let task_message = NatsTaskMessage {
        task_type: "webhook.replay_batch".to_string(),
        task_id: format!(
            "webhook-replay-batch-{}-{}",
            deployment_id,
            chrono::Utc::now().timestamp()
        ),
        payload: task_payload_json,
    };

    // Queue to NATS for background processing
    app_state
        .nats_client
        .publish(
            "worker.tasks.webhook.replay_batch",
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
                format!("Failed to queue replay task: {}", e),
            )
        })?;

    Ok(ReplayWebhookDeliveryResponse {
        status: "queued".to_string(),
        message: "Webhook deliveries queued for replay".to_string(),
    }
    .into())
}

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

    // Fetch payload from S3 if available
    let payload = if !delivery.payload_s3_key.is_empty()
        && !delivery.payload_s3_key.starts_with("endpoint-")
    {
        commands::webhook_storage::RetrieveWebhookPayloadCommand::new(
            delivery.payload_s3_key.clone(),
        )
        .execute(&app_state)
        .await
        .ok()
    } else {
        None
    };

    // Convert WebhookDelivery to WebhookDeliveryDetails
    let delivery_details = dto::json::webhook_requests::WebhookDeliveryDetails {
        delivery_id: delivery.delivery_id,
        deployment_id: delivery.deployment_id,
        app_name: delivery.app_name,
        endpoint_id: delivery.endpoint_id,
        endpoint_url: delivery.endpoint_url,
        event_name: delivery.event_name,
        status: delivery.status,
        http_status_code: delivery.http_status_code,
        response_time_ms: delivery.response_time_ms,
        attempt_number: delivery.attempt_number,
        max_attempts: delivery.max_attempts,
        error_message: delivery.error_message,
        filtered_reason: delivery.filtered_reason,
        payload_s3_key: delivery.payload_s3_key,
        response_body: delivery.response_body,
        response_headers: delivery
            .response_headers
            .and_then(|h| serde_json::from_str(&h).ok()),
        timestamp: delivery.timestamp,
        payload,
    };

    Ok(delivery_details.into())
}

pub async fn reactivate_webhook_endpoint(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(endpoint_id): Path<i64>,
) -> ApiResult<ReactivateEndpointResponse> {
    use commands::webhook_endpoint::ReactivateEndpointCommand;

    // Reactivate the endpoint and clear failure counter
    let endpoint = ReactivateEndpointCommand {
        endpoint_id,
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
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((_app_name, endpoint_id)): Path<(String, i64)>,
    Json(request): Json<TestWebhookEndpointRequest>,
) -> ApiResult<TestWebhookEndpointResponse> {
    use commands::webhook_endpoint::TestWebhookEndpointCommand;

    // Use the payload from the request
    let test_payload = request.payload.unwrap_or_else(|| {
        serde_json::json!({
            "test": true,
            "event": request.event_name,
            "timestamp": chrono::Utc::now()
        })
    });

    let result = TestWebhookEndpointCommand {
        endpoint_id,
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
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
    Query(params): Query<WebhookAnalyticsQuery>,
) -> ApiResult<WebhookAnalyticsResult> {
    let mut query = GetWebhookAnalyticsQuery::new(deployment_id).with_app_name(app_name);

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
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
    Query(params): Query<WebhookTimeseriesQuery>,
) -> ApiResult<WebhookTimeseriesResult> {
    let mut query =
        GetWebhookTimeseriesQuery::new(deployment_id, params.interval).with_app_name(app_name);

    if let Some(endpoint_id) = params.endpoint_id {
        query = query.with_endpoint(endpoint_id);
    }

    if let (Some(start), Some(end)) = (params.start_date, params.end_date) {
        query = query.with_date_range(start, end);
    }

    let result = query.execute(&app_state).await?;

    Ok(result.into())
}

pub async fn get_webhook_stats(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
) -> ApiResult<WebhookAnalyticsResult> {
    let query = GetWebhookAnalyticsQuery::new(deployment_id).with_app_name(app_name);

    let result = query.execute(&app_state).await?;

    Ok(result.into())
}

pub async fn get_app_webhook_deliveries(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
    Query(params): Query<GetAppWebhookDeliveriesQuery>,
) -> ApiResult<PaginatedResponse<WebhookDeliveryListResponse>> {
    let limit = params.limit.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);

    // Fetch one extra to determine if there are more
    let delivery_rows = app_state
        .clickhouse_service
        .get_webhook_deliveries(
            deployment_id,
            Some(app_name),
            params.status.as_deref(),
            params.event_name.as_deref(),
            (limit + 1) as usize,
            offset as usize,
        )
        .await?;

    let has_more = delivery_rows.len() > limit as usize;
    let mut deliveries: Vec<WebhookDeliveryListResponse> =
        delivery_rows.into_iter().map(|row| row.into()).collect();

    if has_more {
        deliveries.truncate(limit as usize);
    }

    Ok(PaginatedResponse {
        data: deliveries,
        has_more,
        limit: Some(limit),
        offset: Some(offset),
    }
    .into())
}
