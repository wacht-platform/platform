// Console-specific webhook management functions
// Webhook apps are stored in the console's database
// Each customer deployment can have one webhook app (named after their deployment_id)

use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;

use crate::application::{HttpState, response::ApiResult};
use crate::middleware::{ConsoleDeployment, RequireDeployment};
use commands::{
    Command,
    webhook_app::{CreateWebhookAppCommand, RotateWebhookSecretCommand},
    webhook_endpoint::{
        CreateWebhookEndpointCommand, DeleteWebhookEndpointCommand, EventSubscriptionData,
        UpdateWebhookEndpointCommand,
    },
};
use dto::json::{
    WebhookStatus,
    webhook_requests::{
        ConsoleAnalyticsQuery, ConsoleTimeseriesQuery, CreateWebhookEndpointConsoleRequest,
        DeliveryListQuery, GetAvailableEventsResponse, ListWebhookEndpointsQuery,
        ListWebhookEndpointsResponse, TestWebhookRequest, UpdateWebhookEndpointRequest,
    },
};
use models::{
    webhook::{WebhookApp, WebhookEndpoint, WebhookEventDefinition},
    webhook_analytics::TimeseriesInterval,
};
use queries::{
    Query as QueryTrait,
    webhook::{GetWebhookAppByNameQuery, GetWebhookEndpointsQuery, GetWebhookStatsQuery},
    webhook_analytics::{GetWebhookAnalyticsQuery, GetWebhookTimeseriesQuery},
};

// Get webhook status for a deployment
pub async fn get_webhook_status(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<WebhookStatus> {
    let app_name = deployment_id.to_string();

    let app = GetWebhookAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?;

    let stats = if let Some(ref app) = app {
        match GetWebhookStatsQuery::new(console_deployment_id, app.name.clone())
            .execute(&app_state)
            .await
        {
            Ok(stats) => Some(stats),
            Err(e) => {
                eprintln!("Failed to get webhook stats: {:?}", e);
                // Return empty stats when ClickHouse query fails (likely no data yet)
                Some(dto::json::WebhookStats {
                    total_deliveries: 0,
                    success_rate: 0.0,
                    active_endpoints: 0,
                    failed_deliveries_24h: 0,
                })
            }
        }
    } else {
        None
    };

    Ok(WebhookStatus {
        is_activated: app.is_some(),
        app,
        stats,
    }
    .into())
}

pub async fn activate_webhooks(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<WebhookApp> {
    let app_name = deployment_id.to_string();

    let mut command = CreateWebhookAppCommand::new(console_deployment_id, app_name);

    let platform_events = vec![
        WebhookEventDefinition {
            name: "user.created".to_string(),
            description: "New user signed up".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "user.updated".to_string(),
            description: "User profile updated".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "user.deleted".to_string(),
            description: "User account deleted".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "user.email.verified".to_string(),
            description: "User email address verified".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "user.password.updated".to_string(),
            description: "User password changed".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "user.mfa.enabled".to_string(),
            description: "User enabled two-factor authentication".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "user.mfa.disabled".to_string(),
            description: "User disabled two-factor authentication".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "session.created".to_string(),
            description: "User signed in".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "session.deleted".to_string(),
            description: "User signed out".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "session.expired".to_string(),
            description: "User session expired".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "organization.created".to_string(),
            description: "New organization created".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "organization.updated".to_string(),
            description: "Organization settings updated".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "organization.deleted".to_string(),
            description: "Organization deleted".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "organization.member.added".to_string(),
            description: "Member added to organization".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "organization.member.removed".to_string(),
            description: "Member removed from organization".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "organization.member.role.updated".to_string(),
            description: "Organization member role changed".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "workspace.created".to_string(),
            description: "New workspace created".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "workspace.updated".to_string(),
            description: "Workspace settings updated".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "workspace.deleted".to_string(),
            description: "Workspace deleted".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "workspace.member.added".to_string(),
            description: "Member added to workspace".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "workspace.member.removed".to_string(),
            description: "Member removed from workspace".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "api_key.created".to_string(),
            description: "API key created".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "api_key.deleted".to_string(),
            description: "API key deleted".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "api_key.rotated".to_string(),
            description: "API key rotated".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "agent.created".to_string(),
            description: "AI agent created".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "agent.updated".to_string(),
            description: "AI agent configuration updated".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "agent.deleted".to_string(),
            description: "AI agent deleted".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "agent.execution.started".to_string(),
            description: "AI agent execution started".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "agent.execution.completed".to_string(),
            description: "AI agent execution completed successfully".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "agent.execution.failed".to_string(),
            description: "AI agent execution failed".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "waitlist.entry.created".to_string(),
            description: "New waitlist entry added".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "waitlist.entry.approved".to_string(),
            description: "Waitlist entry approved".to_string(),
            schema: None,
        },
    ];

    command = command.with_events(platform_events);

    let app = command.execute(&app_state).await?;
    Ok(app.into())
}

pub async fn rotate_webhook_secret(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<WebhookApp> {
    let app_name = deployment_id.to_string();

    let command = RotateWebhookSecretCommand {
        deployment_id: console_deployment_id,
        app_name,
    };

    let app = command.execute(&app_state).await?;
    Ok(app.into())
}

pub async fn list_webhook_endpoints(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ListWebhookEndpointsQuery>,
) -> ApiResult<ListWebhookEndpointsResponse> {
    let app_name = deployment_id.to_string();

    let app = GetWebhookAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "Webhook app not found. Please activate webhooks first.",
            )
        })?;

    let include_inactive = params.include_inactive.unwrap_or(false);
    let endpoints = GetWebhookEndpointsQuery::new(console_deployment_id)
        .with_inactive(include_inactive)
        .for_app(app.name)
        .execute(&app_state)
        .await?;

    Ok(ListWebhookEndpointsResponse {
        total: endpoints.len(),
        endpoints,
    }
    .into())
}

// Create a webhook endpoint
pub async fn create_webhook_endpoint(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateWebhookEndpointConsoleRequest>,
) -> ApiResult<WebhookEndpoint> {
    let app_name = deployment_id.to_string();

    let app = GetWebhookAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "Webhook app not found. Please activate webhooks first.",
            )
        })?;

    let subscriptions: Vec<EventSubscriptionData> = request
        .subscriptions
        .into_iter()
        .map(|sub| EventSubscriptionData {
            event_name: sub.event_name,
            filter_rules: sub.filter_rules,
        })
        .collect();

    let command = CreateWebhookEndpointCommand {
        deployment_id: console_deployment_id,
        app_name: app.name,
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

// Update a webhook endpoint
pub async fn update_webhook_endpoint(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(_deployment_id): RequireDeployment,
    Path((_deployment_id_path, endpoint_id)): Path<(i64, i64)>,
    Json(request): Json<UpdateWebhookEndpointRequest>,
) -> ApiResult<WebhookEndpoint> {
    let command = UpdateWebhookEndpointCommand {
        endpoint_id,
        deployment_id: console_deployment_id,
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

// Delete a webhook endpoint
pub async fn delete_webhook_endpoint(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(_deployment_id): RequireDeployment,
    Path((_deployment_id_path, endpoint_id)): Path<(i64, i64)>,
) -> ApiResult<()> {
    let command = DeleteWebhookEndpointCommand {
        endpoint_id,
        deployment_id: console_deployment_id,
    };

    command.execute(&app_state).await?;
    Ok(().into())
}

// Analytics endpoints
pub async fn get_webhook_analytics(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ConsoleAnalyticsQuery>,
) -> ApiResult<serde_json::Value> {
    let app_name = deployment_id.to_string();

    // Get the app to find its ID
    let app = GetWebhookAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?;

    if let Some(app) = app {
        let mut query =
            GetWebhookAnalyticsQuery::new(console_deployment_id).with_app_name(app.name);

        if let (Some(start), Some(end)) = (params.start_date, params.end_date) {
            query = query.with_date_range(start, end);
        }

        let result = query.execute(&app_state).await?;
        Ok(serde_json::to_value(result).unwrap().into())
    } else {
        Ok(serde_json::json!({
            "total_events": 0,
            "total_deliveries": 0,
            "successful_deliveries": 0,
            "failed_deliveries": 0,
            "filtered_deliveries": 0,
            "success_rate": 0.0,
            "top_events": [],
            "endpoint_performance": [],
            "failure_reasons": []
        })
        .into())
    }
}

pub async fn get_webhook_timeseries(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ConsoleTimeseriesQuery>,
) -> ApiResult<serde_json::Value> {
    let app_name = deployment_id.to_string();

    eprintln!(
        "get_webhook_timeseries: deployment_id={}, console_deployment_id={}, app_name={}",
        deployment_id, console_deployment_id, app_name
    );
    eprintln!(
        "Query params: start={:?}, end={:?}, interval={}",
        params.start_date, params.end_date, params.interval
    );

    // Get the app to find its ID
    let app = match GetWebhookAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await
    {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Failed to get webhook app: {:?}", e);
            return Err(e.into());
        }
    };

    if let Some(app) = app {
        eprintln!("Found app: name={}", app.name);

        let interval = match params.interval.as_str() {
            "minute" => TimeseriesInterval::Minute,
            "hour" => TimeseriesInterval::Hour,
            "day" => TimeseriesInterval::Day,
            _ => TimeseriesInterval::Hour,
        };

        let mut query = GetWebhookTimeseriesQuery::new(console_deployment_id, interval)
            .with_app_name(app.name.clone());

        if let (Some(start), Some(end)) = (params.start_date, params.end_date) {
            query = query.with_date_range(start, end);
        }

        eprintln!("Executing timeseries query with app_name={}", app.name);

        match query.execute(&app_state).await {
            Ok(result) => {
                eprintln!(
                    "Timeseries query successful, got {} data points",
                    result.data.len()
                );
                match serde_json::to_value(result) {
                    Ok(json) => Ok(json.into()),
                    Err(e) => {
                        eprintln!("Failed to serialize result to JSON: {:?}", e);
                        Err((
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Failed to serialize response",
                        )
                            .into())
                    }
                }
            }
            Err(e) => {
                eprintln!("Timeseries query failed: {:?}", e);
                Err(e.into())
            }
        }
    } else {
        eprintln!("No app found for deployment_id={}", deployment_id);
        Ok(serde_json::json!({
            "data": [],
            "interval": params.interval
        })
        .into())
    }
}

// Delivery history endpoints
pub async fn get_webhook_deliveries(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<DeliveryListQuery>,
) -> ApiResult<serde_json::Value> {
    let app_name = deployment_id.to_string();

    // Get the app to find its ID
    let app = GetWebhookAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?;

    if let Some(app) = app {
        // Get recent deliveries from ClickHouse
        let limit = params.limit.unwrap_or(100) as usize;
        let deliveries = app_state
            .clickhouse_service
            .get_recent_webhook_deliveries(
                console_deployment_id,
                Some(app.name),
                params.status.as_deref(),
                params.event_name.as_deref(),
                limit,
            )
            .await?;

        Ok(serde_json::json!({
            "deliveries": deliveries
        })
        .into())
    } else {
        Ok(serde_json::json!({
            "deliveries": []
        })
        .into())
    }
}

pub async fn get_webhook_delivery_details(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(_deployment_id): RequireDeployment,
    Path((_deployment_id_path, delivery_id)): Path<(i64, i64)>,
) -> ApiResult<serde_json::Value> {
    // Get delivery details from ClickHouse (using console deployment ID as that's where webhooks are stored)
    let details = app_state
        .clickhouse_service
        .get_webhook_delivery_details(console_deployment_id, delivery_id)
        .await?;

    Ok(serde_json::to_value(details).unwrap().into())
}

pub async fn retry_webhook_delivery(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(_deployment_id): RequireDeployment,
    Path((_deployment_id_path, delivery_id)): Path<(i64, i64)>,
) -> ApiResult<serde_json::Value> {
    use dto::json::nats::NatsTaskMessage;

    // Queue retry task for background processing
    let task_message = NatsTaskMessage {
        task_type: "webhook.retry".to_string(),
        task_id: format!("webhook-retry-{}-{}", delivery_id, console_deployment_id),
        payload: serde_json::json!({
            "delivery_id": delivery_id,
            "deployment_id": console_deployment_id
        }),
        retry_count: 0,
        max_retries: 3,
    };

    app_state
        .nats_client
        .publish(
            "worker.tasks.webhook.retry",
            serde_json::to_vec(&task_message)
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to serialize task: {}", e),
                    )
                })?
                .into(),
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to queue retry task: {}", e),
            )
        })?;

    Ok(serde_json::json!({
        "message": "Webhook retry has been queued",
        "delivery_id": delivery_id
    })
    .into())
}

// Test webhook endpoint
pub async fn test_webhook_endpoint(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(_deployment_id): RequireDeployment,
    Path((_, endpoint_id)): Path<(i64, i64)>,
    Json(request): Json<TestWebhookRequest>,
) -> ApiResult<serde_json::Value> {
    use commands::webhook_endpoint::TestWebhookEndpointCommand;

    // Send a test request to the endpoint
    let result = TestWebhookEndpointCommand {
        deployment_id: console_deployment_id,
        endpoint_id,
        test_payload: request.payload.unwrap_or_else(|| {
            serde_json::json!({
                "event": request.event_name,
                "test": true,
                "timestamp": chrono::Utc::now().to_rfc3339()
            })
        }),
    }
    .execute(&app_state)
    .await?;

    Ok(serde_json::json!({
        "status_code": result.status_code,
        "response_time_ms": result.response_time_ms,
        "success": result.success,
        "delivery_id": result.delivery_id,
        "message": if result.success { "Test webhook sent successfully" } else { "Test webhook failed" }
    }).into())
}

// Reactivate webhook endpoint
pub async fn reactivate_webhook_endpoint(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(_deployment_id): RequireDeployment,
    Path((_deployment_id_path, endpoint_id)): Path<(i64, i64)>,
) -> ApiResult<serde_json::Value> {
    use commands::webhook_endpoint::ReactivateEndpointCommand;

    ReactivateEndpointCommand {
        deployment_id: console_deployment_id,
        endpoint_id,
    }
    .execute(&app_state)
    .await?;

    Ok(serde_json::json!({
        "message": "Webhook endpoint reactivated successfully"
    })
    .into())
}

// Get available events for a deployment
pub async fn get_available_events(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<GetAvailableEventsResponse> {
    use queries::webhook::GetWebhookEventsQuery;

    let app_name = deployment_id.to_string();

    // Get the app first
    let app = GetWebhookAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "Webhook app not found. Please activate webhooks first.",
            )
        })?;

    // Get events for this app
    let events = GetWebhookEventsQuery::new(console_deployment_id, app.name)
        .execute(&app_state)
        .await?;

    Ok(GetAvailableEventsResponse { events }.into())
}
