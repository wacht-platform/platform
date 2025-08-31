// Console-specific webhook management functions
// These functions use the SDK to call backend API endpoints

use crate::application::response::{ApiResult, PaginatedResponse};
use crate::middleware::RequireDeployment;
use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;
use dto::clickhouse::webhook::WebhookDeliveryListResponse;
use dto::json::{
    WebhookStatus,
    webhook_requests::{
        ConsoleAnalyticsQuery, ConsoleTimeseriesQuery, CreateWebhookEndpointConsoleRequest,
        DeliveryListQuery, GetAvailableEventsResponse, ListWebhookEndpointsQuery,
        ReplayWebhookDeliveryRequest, ReplayWebhookDeliveryResponse,
        TestWebhookRequest, TestWebhookEndpointResponse, UpdateWebhookEndpointRequest,
        WebhookEndpoint as WebhookEndpointDto, WebhookEndpointSubscription,
    },
};
use models::webhook::{WebhookApp, WebhookEndpoint, WebhookEventDefinition};
use models::webhook_analytics::{WebhookAnalyticsResult, WebhookTimeseriesResult};
use queries::Query as QueryTrait;
use wacht::api::webhooks;

// Helper function to convert SDK WebhookDelivery to DTO WebhookDeliveryListResponse
fn convert_sdk_delivery(delivery: wacht::api::webhooks::WebhookDelivery) -> WebhookDeliveryListResponse {
    WebhookDeliveryListResponse {
        delivery_id: delivery.delivery_id.parse().unwrap_or(0),
        deployment_id: delivery.deployment_id.parse().unwrap_or(0),
        app_name: delivery.app_name,
        endpoint_id: delivery.endpoint_id.parse().unwrap_or(0),
        endpoint_url: delivery.endpoint_url,
        event_name: delivery.event_name,
        status: delivery.status,
        http_status_code: delivery.http_status_code,
        response_time_ms: delivery.response_time_ms,
        attempt_number: delivery.attempt_number,
        max_attempts: delivery.max_attempts,
        error_message: delivery.error_message,
        filtered_reason: delivery.filtered_reason,
        timestamp: delivery.timestamp,
    }
}

// Get webhook status for a deployment
pub async fn get_webhook_status(
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<WebhookStatus> {
    let app_name = deployment_id.to_string();

    // Try to get app using SDK
    let app = webhooks::get_webhook_app(&app_name)
        .await
        .ok()
        .map(|app_data| WebhookApp {
            deployment_id: app_data.deployment_id.parse().unwrap_or(deployment_id),
            name: app_data.name,
            description: app_data.description,
            signing_secret: app_data.signing_secret,
            is_active: app_data.is_active,
            created_at: app_data.created_at,
            updated_at: app_data.updated_at,
        });

    let stats = if let Some(ref app) = app {
        match webhooks::get_webhook_stats(&app.name).await {
            Ok(stats_data) => Some(dto::json::WebhookStats {
                total_deliveries: stats_data.total_deliveries,
                success_rate: stats_data.success_rate,
                active_endpoints: stats_data.endpoint_performance.len() as i64,
                failed_deliveries_24h: stats_data.failed_deliveries,
            }),
            Err(e) => {
                eprintln!("Failed to get webhook stats: {:?}", e);
                // Return empty stats when query fails (likely no data yet)
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
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<WebhookApp> {
    let app_name = deployment_id.to_string();

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
        WebhookEventDefinition {
            name: "execution_context.message".to_string(),
            description: "Message sent in execution context".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "execution_context.platform_event".to_string(),
            description: "Platform event occurred in execution context".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "execution_context.platform_function".to_string(),
            description: "Platform function called in execution context".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "execution_context.user_input_request".to_string(),
            description: "User input requested in execution context".to_string(),
            schema: None,
        },
    ];

    let sdk_events: Vec<webhooks::WebhookEventDefinition> = platform_events
        .iter()
        .map(|e| webhooks::WebhookEventDefinition {
            name: e.name.clone(),
            description: e.description.clone(),
            schema: e.schema.clone(),
        })
        .collect();

    let request = webhooks::CreateWebhookAppRequest {
        name: app_name,
        description: Some(format!("Webhooks for deployment {}", deployment_id)),
        is_active: Some(true),
        events: Some(sdk_events),
    };

    let app_data = webhooks::create_webhook_app(request)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let app = WebhookApp {
        deployment_id: app_data.deployment_id.parse().unwrap_or(0),
        name: app_data.name,
        description: app_data.description,
        signing_secret: app_data.signing_secret,
        is_active: app_data.is_active,
        created_at: app_data.created_at,
        updated_at: app_data.updated_at,
    };

    Ok(app.into())
}

pub async fn rotate_webhook_secret(
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<WebhookApp> {
    let app_name = deployment_id.to_string();

    let app_data = webhooks::rotate_webhook_secret(&app_name)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let app = WebhookApp {
        deployment_id: app_data.deployment_id.parse().unwrap_or(0),
        name: app_data.name,
        description: app_data.description,
        signing_secret: app_data.signing_secret,
        is_active: app_data.is_active,
        created_at: app_data.created_at,
        updated_at: app_data.updated_at,
    };

    Ok(app.into())
}

pub async fn list_webhook_endpoints(
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ListWebhookEndpointsQuery>,
) -> ApiResult<PaginatedResponse<WebhookEndpointDto>> {
    let app_name = deployment_id.to_string();

    // Check if app exists first
    webhooks::get_webhook_app(&app_name).await.map_err(|err| {
        eprintln!("Error getting webhook app: {}", err);
        (
            StatusCode::NOT_FOUND,
            "Webhook app not found. Please activate webhooks first.",
        )
    })?;

    let include_inactive = params.include_inactive.unwrap_or(false);
    let paginated_response = webhooks::get_webhook_endpoints_with_subscriptions(
        &app_name, 
        Some(include_inactive),
        params.limit,
        params.offset,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Convert to dto types
    let endpoints: Vec<WebhookEndpointDto> = paginated_response.endpoints
        .into_iter()
        .map(|e| {
            // Convert subscribed events to subscription structs
            let subscriptions: Vec<WebhookEndpointSubscription> = e
                .subscribed_events
                .into_iter()
                .map(|event_name| {
                    WebhookEndpointSubscription {
                        event_name,
                        filter_rules: None, // These will be populated if needed
                    }
                })
                .collect();

            WebhookEndpointDto {
                id: e.endpoint.id.parse().unwrap_or(0),
                deployment_id: e.endpoint.deployment_id.parse().unwrap_or(0),
                app_name: e.endpoint.app_name,
                url: e.endpoint.url,
                description: e.endpoint.description,
                headers: e.endpoint.headers,
                is_active: e.endpoint.is_active,
                signing_secret: e.endpoint.signing_secret,
                max_retries: e.endpoint.max_retries,
                timeout_seconds: e.endpoint.timeout_seconds,
                failure_count: e.endpoint.failure_count,
                last_failure_at: e.endpoint.last_failure_at,
                auto_disabled: e.endpoint.auto_disabled,
                auto_disabled_at: e.endpoint.auto_disabled_at,
                created_at: e.endpoint.created_at,
                updated_at: e.endpoint.updated_at,
                subscriptions,
            }
        })
        .collect();

    Ok(PaginatedResponse {
        data: endpoints,
        has_more: paginated_response.has_more,
        limit: Some(paginated_response.limit),
        offset: Some(paginated_response.offset),
    }
    .into())
}

// Create a webhook endpoint
pub async fn create_webhook_endpoint(
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateWebhookEndpointConsoleRequest>,
) -> ApiResult<WebhookEndpoint> {
    let app_name = deployment_id.to_string();

    // Check if app exists first
    webhooks::get_webhook_app(&app_name).await.map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            "Webhook app not found. Please activate webhooks first.",
        )
    })?;

    let subscriptions: Vec<webhooks::EventSubscription> = request
        .subscriptions
        .into_iter()
        .map(|sub| webhooks::EventSubscription {
            event_name: sub.event_name,
            filter_rules: sub.filter_rules,
        })
        .collect();

    let sdk_request = webhooks::CreateWebhookEndpointRequest {
        app_name,
        url: request.url,
        description: request.description,
        headers: request.headers,
        subscriptions,
        max_retries: request.max_retries,
        timeout_seconds: request.timeout_seconds,
    };

    let endpoint_data = webhooks::create_webhook_endpoint(sdk_request)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let endpoint = WebhookEndpoint {
        id: endpoint_data.id.parse().unwrap_or(0),
        deployment_id: endpoint_data.deployment_id.parse().unwrap_or(0),
        app_name: endpoint_data.app_name,
        url: endpoint_data.url,
        description: endpoint_data.description,
        headers: endpoint_data.headers,
        is_active: endpoint_data.is_active,
        signing_secret: endpoint_data.signing_secret,
        max_retries: endpoint_data.max_retries,
        timeout_seconds: endpoint_data.timeout_seconds,
        failure_count: endpoint_data.failure_count,
        last_failure_at: endpoint_data.last_failure_at,
        auto_disabled: endpoint_data.auto_disabled,
        auto_disabled_at: endpoint_data.auto_disabled_at,
        created_at: endpoint_data.created_at,
        updated_at: endpoint_data.updated_at,
    };

    Ok(endpoint.into())
}

// Update a webhook endpoint
pub async fn update_webhook_endpoint(
    Path((_deployment_id_path, endpoint_id)): Path<(i64, i64)>,
    Json(request): Json<UpdateWebhookEndpointRequest>,
) -> ApiResult<WebhookEndpoint> {
    let sdk_request = webhooks::UpdateWebhookEndpointRequest {
        url: request.url,
        description: request.description,
        headers: request.headers,
        is_active: request.is_active,
        max_retries: request.max_retries,
        timeout_seconds: request.timeout_seconds,
        subscriptions: request.subscriptions.map(|subs| {
            subs.into_iter().map(Into::into).collect()
        }),
    };

    let endpoint_data = webhooks::update_webhook_endpoint(endpoint_id.to_string(), sdk_request)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let endpoint = WebhookEndpoint {
        id: endpoint_data.id.parse().unwrap_or(0),
        deployment_id: endpoint_data.deployment_id.parse().unwrap_or(0),
        app_name: endpoint_data.app_name,
        url: endpoint_data.url,
        description: endpoint_data.description,
        headers: endpoint_data.headers,
        is_active: endpoint_data.is_active,
        signing_secret: endpoint_data.signing_secret,
        max_retries: endpoint_data.max_retries,
        timeout_seconds: endpoint_data.timeout_seconds,
        failure_count: endpoint_data.failure_count,
        last_failure_at: endpoint_data.last_failure_at,
        auto_disabled: endpoint_data.auto_disabled,
        auto_disabled_at: endpoint_data.auto_disabled_at,
        created_at: endpoint_data.created_at,
        updated_at: endpoint_data.updated_at,
    };

    Ok(endpoint.into())
}

// Delete a webhook endpoint
pub async fn delete_webhook_endpoint(
    Path((_deployment_id_path, endpoint_id)): Path<(i64, i64)>,
) -> ApiResult<()> {
    webhooks::delete_webhook_endpoint(endpoint_id.to_string())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(().into())
}

// Analytics endpoints
pub async fn get_webhook_analytics(
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ConsoleAnalyticsQuery>,
) -> ApiResult<WebhookAnalyticsResult> {
    let app_name = deployment_id.to_string();

    // Check if app exists
    let app_exists = webhooks::get_webhook_app(&app_name).await.is_ok();

    if app_exists {
        let start_date = params.start_date.map(|d| d.to_rfc3339());
        let end_date = params.end_date.map(|d| d.to_rfc3339());

        let sdk_result = webhooks::get_webhook_analytics(
            &app_name,
            start_date.as_deref(),
            end_date.as_deref(),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        // Convert SDK result to platform model
        Ok(WebhookAnalyticsResult {
            total_events: sdk_result.total_events,
            total_deliveries: sdk_result.total_deliveries,
            successful_deliveries: sdk_result.successful_deliveries,
            failed_deliveries: sdk_result.failed_deliveries,
            filtered_deliveries: sdk_result.filtered_deliveries,
            avg_response_time_ms: sdk_result.avg_response_time_ms,
            p50_response_time_ms: sdk_result.p50_response_time_ms,
            p95_response_time_ms: sdk_result.p95_response_time_ms,
            p99_response_time_ms: sdk_result.p99_response_time_ms,
            success_rate: sdk_result.success_rate,
            top_events: sdk_result.top_events.into_iter()
                .map(|e| models::webhook_analytics::EventCount {
                    event_name: e.event_name,
                    count: e.count,
                })
                .collect(),
            endpoint_performance: sdk_result.endpoint_performance.into_iter()
                .map(|e| models::webhook_analytics::EndpointPerformance {
                    endpoint_id: e.endpoint_id,
                    endpoint_url: e.endpoint_url,
                    total_attempts: e.total_attempts,
                    successful_attempts: e.successful_attempts,
                    failed_attempts: e.failed_attempts,
                    avg_response_time_ms: e.avg_response_time_ms,
                    success_rate: e.success_rate,
                })
                .collect(),
            failure_reasons: sdk_result.failure_reasons.into_iter()
                .map(|f| models::webhook_analytics::FailureReason {
                    reason: f.reason,
                    count: f.count,
                })
                .collect(),
        }.into())
    } else {
        // Return empty analytics result
        Ok(WebhookAnalyticsResult {
            total_events: 0,
            total_deliveries: 0,
            successful_deliveries: 0,
            failed_deliveries: 0,
            filtered_deliveries: 0,
            avg_response_time_ms: None,
            p50_response_time_ms: None,
            p95_response_time_ms: None,
            p99_response_time_ms: None,
            success_rate: 0.0,
            top_events: vec![],
            endpoint_performance: vec![],
            failure_reasons: vec![],
        }.into())
    }
}

pub async fn get_webhook_timeseries(
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ConsoleTimeseriesQuery>,
) -> ApiResult<WebhookTimeseriesResult> {
    let app_name = deployment_id.to_string();

    let app_exists = match webhooks::get_webhook_app(&app_name).await {
        Ok(_) => true,
        Err(_) => false,
    };

    if app_exists {
        let start_date = params.start_date.map(|d| d.to_rfc3339());
        let end_date = params.end_date.map(|d| d.to_rfc3339());

        let sdk_result = webhooks::get_webhook_timeseries(
            &app_name,
            &params.interval,
            start_date.as_deref(),
            end_date.as_deref(),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        // Convert SDK result to platform model
        Ok(WebhookTimeseriesResult {
            data: sdk_result.data.into_iter()
                .map(|p| models::webhook_analytics::TimeseriesPoint {
                    timestamp: p.timestamp,
                    total_events: p.total_events,
                    total_deliveries: p.total_deliveries,
                    successful_deliveries: p.successful_deliveries,
                    failed_deliveries: p.failed_deliveries,
                    filtered_deliveries: p.filtered_deliveries,
                    avg_response_time_ms: p.avg_response_time_ms,
                    success_rate: p.success_rate,
                })
                .collect(),
            interval: sdk_result.interval,
        }.into())
    } else {
        // Return empty timeseries result
        Ok(WebhookTimeseriesResult {
            data: vec![],
            interval: params.interval,
        }.into())
    }
}

pub async fn get_webhook_deliveries(
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<DeliveryListQuery>,
) -> ApiResult<PaginatedResponse<WebhookDeliveryListResponse>> {
    let app_name = deployment_id.to_string();

    // Check if app exists
    let app_exists = webhooks::get_webhook_app(&app_name).await.is_ok();

    if app_exists {
        let sdk_result = webhooks::get_webhook_deliveries(
            &app_name,
            None, // endpoint_id
            params.event_name.as_deref(),
            params.status.as_deref(),
            params.limit.map(|l| l as i32),
            params.offset.map(|o| o as i32),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        // Convert SDK response to our DTO
        let deliveries = sdk_result.data.into_iter()
            .map(convert_sdk_delivery)
            .collect();

        Ok(PaginatedResponse {
            data: deliveries,
            has_more: sdk_result.has_more,
            limit: sdk_result.limit,
            offset: sdk_result.offset,
        }.into())
    } else {
        Ok(PaginatedResponse {
            data: vec![],
            has_more: false,
            limit: Some(params.limit.unwrap_or(100)),
            offset: Some(params.offset.unwrap_or(0)),
        }.into())
    }
}

pub async fn get_webhook_delivery_details(
    Path((_deployment_id_path, delivery_id)): Path<(i64, i64)>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> ApiResult<serde_json::Value> {
    // Get status from query params if provided
    let status = params.get("status").map(|s| s.as_str());
    
    let details = webhooks::get_webhook_delivery_details(delivery_id.to_string(), status)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(serde_json::to_value(details)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .into())
}

// Replay webhook deliveries
pub async fn replay_webhook_deliveries(
    State(app_state): State<common::state::AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<ReplayWebhookDeliveryRequest>,
) -> ApiResult<ReplayWebhookDeliveryResponse> {
    // Get the app name for this deployment
    let apps = queries::webhook::GetWebhookAppsQuery::new(deployment_id)
        .execute(&app_state)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, "Failed to get webhook apps"))?;
    
    let app = apps.first()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "No webhook app found for deployment"))?;

    // Handle the request based on type
    match request {
        ReplayWebhookDeliveryRequest::ByIds { delivery_ids, include_successful } => {
            // Delivery IDs are already strings (for Snowflake ID compatibility)
            webhooks::replay_webhook_deliveries(app.name.clone(), delivery_ids, include_successful)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
        ReplayWebhookDeliveryRequest::ByDateRange { .. } => {
            // Date range replay not supported through SDK yet
            return Err((StatusCode::BAD_REQUEST, "Date range replay not yet supported through console").into());
        }
    }

    Ok(ReplayWebhookDeliveryResponse {
        status: "queued".to_string(),
        message: "Webhook deliveries queued for replay".to_string(),
    }.into())
}

// Test webhook endpoint directly
pub async fn test_webhook_endpoint(
    RequireDeployment(deployment_id): RequireDeployment,
    Path((_deployment_id_path, endpoint_id)): Path<(i64, i64)>,
    Json(request): Json<TestWebhookRequest>,
) -> ApiResult<TestWebhookEndpointResponse> {
    let app_name = deployment_id.to_string();
    
    // Call SDK's test endpoint
    let result = webhooks::test_webhook_endpoint(
        &app_name,
        endpoint_id.to_string(),
        request.event_name,
        request.payload,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    // Return strongly typed response
    Ok(TestWebhookEndpointResponse {
        success: result.success,
        status_code: result.status_code as u16,
        response_time_ms: result.response_time_ms as u32,
        response_body: result.response_body,
        error: result.error_message,
    }.into())
}

// Reactivate webhook endpoint
pub async fn reactivate_webhook_endpoint(
    Path((_deployment_id_path, endpoint_id)): Path<(i64, i64)>,
) -> ApiResult<serde_json::Value> {
    let result = webhooks::reactivate_webhook_endpoint(endpoint_id.to_string())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(result.into())
}

// Get available events for a deployment
pub async fn get_available_events(
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<GetAvailableEventsResponse> {
    let app_name = deployment_id.to_string();

    // Check if app exists first
    webhooks::get_webhook_app(&app_name).await.map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            "Webhook app not found. Please activate webhooks first.",
        )
    })?;

    // Get events for this app - pass through SDK data directly
    let events = webhooks::get_webhook_events(&app_name)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(GetAvailableEventsResponse { events }.into())
}
