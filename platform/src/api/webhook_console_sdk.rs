// Console-specific webhook management using the Wacht SDK
// This is an example of how console endpoints can use the SDK instead of direct commands

use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;

use crate::application::{HttpState, response::ApiResult};
use crate::middleware::{ConsoleDeployment, RequireDeployment};
use dto::json::{
    WebhookStatus,
    webhook_requests::{
        ConsoleAnalyticsQuery, ConsoleTimeseriesQuery, CreateWebhookEndpointConsoleRequest,
        DeliveryListQuery, GetAvailableEventsResponse, ListWebhookEndpointsQuery,
        ListWebhookEndpointsResponse, UpdateWebhookEndpointRequest,
    },
};
use models::webhook::{WebhookApp, WebhookEndpoint, WebhookEventDefinition};

// Example: Get webhook status using SDK
pub async fn get_webhook_status(
    State(_app_state): State<HttpState>,
    ConsoleDeployment(_console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<WebhookStatus> {
    let app_name = deployment_id.to_string();
    
    // Use SDK to get webhook apps
    let apps = wacht::webhooks::list_webhook_apps(Some(false))
        .await
        .map_err(|e| {
            tracing::error!("Failed to get webhook apps via SDK: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get webhook status")
        })?;
    
    // Find the app for this deployment
    let app = apps.iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(&app_name))
        .cloned();
    
    // Get stats if app exists
    let stats = if app.is_some() {
        // In a real implementation, you'd call a stats endpoint
        // For now, return mock stats
        Some(dto::json::WebhookStats {
            total_deliveries: 0,
            success_rate: 0.0,
            active_endpoints: 0,
            failed_deliveries_24h: 0,
        })
    } else {
        None
    };
    
    Ok(WebhookStatus {
        is_activated: app.is_some(),
        app: app.and_then(|a| serde_json::from_value(a).ok()),
        stats,
    }
    .into())
}

// Example: Activate webhooks using SDK
pub async fn activate_webhooks(
    State(_app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<WebhookApp> {
    let app_name = deployment_id.to_string();
    
    let request = wacht::webhooks::CreateWebhookAppRequest {
        name: app_name,
        description: Some(format!("Webhook app for deployment {}", deployment_id)),
        is_active: Some(true),
    };
    
    let app = wacht::webhooks::create_webhook_app(request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to create webhook app via SDK: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to activate webhooks")
        })?;
    
    serde_json::from_value(app)
        .map(Into::into)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Invalid response format").into())
}

// Example: Create webhook endpoint using SDK
pub async fn create_webhook_endpoint(
    State(_app_state): State<HttpState>,
    ConsoleDeployment(_console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateWebhookEndpointConsoleRequest>,
) -> ApiResult<WebhookEndpoint> {
    let app_name = deployment_id.to_string();
    
    let subscriptions = request.subscribe_to_events
        .into_iter()
        .map(|event| wacht::webhooks::EventSubscription {
            event_name: event,
            filter_rules: request.filter_rules.clone(),
        })
        .collect();
    
    let sdk_request = wacht::webhooks::CreateWebhookEndpointRequest {
        app_name,
        url: request.url,
        description: request.description,
        headers: request.headers,
        subscriptions,
        max_retries: request.max_retries,
        timeout_seconds: request.timeout_seconds,
    };
    
    let endpoint = wacht::webhooks::create_webhook_endpoint(sdk_request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to create webhook endpoint via SDK: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create webhook endpoint")
        })?;
    
    serde_json::from_value(endpoint)
        .map(Into::into)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Invalid response format").into())
}

// Example: Update webhook endpoint using SDK
pub async fn update_webhook_endpoint(
    State(_app_state): State<HttpState>,
    ConsoleDeployment(_console_deployment_id): ConsoleDeployment,
    RequireDeployment(_deployment_id): RequireDeployment,
    Path(endpoint_id): Path<i64>,
    Json(request): Json<UpdateWebhookEndpointRequest>,
) -> ApiResult<WebhookEndpoint> {
    let sdk_request = wacht::webhooks::UpdateWebhookEndpointRequest {
        url: request.url,
        description: request.description,
        headers: request.headers,
        is_active: request.is_active,
        max_retries: request.max_retries,
        timeout_seconds: request.timeout_seconds,
    };
    
    let endpoint = wacht::webhooks::update_webhook_endpoint(endpoint_id, sdk_request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to update webhook endpoint via SDK: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to update webhook endpoint")
        })?;
    
    serde_json::from_value(endpoint)
        .map(Into::into)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Invalid response format").into())
}

// Example: Delete webhook endpoint using SDK
pub async fn delete_webhook_endpoint(
    State(_app_state): State<HttpState>,
    ConsoleDeployment(_console_deployment_id): ConsoleDeployment,
    RequireDeployment(_deployment_id): RequireDeployment,
    Path(endpoint_id): Path<i64>,
) -> ApiResult<()> {
    wacht::webhooks::delete_webhook_endpoint(endpoint_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete webhook endpoint via SDK: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete webhook endpoint")
        })?;
    
    Ok(().into())
}

// Example: List webhook endpoints using SDK
pub async fn list_webhook_endpoints(
    State(_app_state): State<HttpState>,
    ConsoleDeployment(_console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<ListWebhookEndpointsQuery>,
) -> ApiResult<ListWebhookEndpointsResponse> {
    let app_name = deployment_id.to_string();
    
    let endpoints = wacht::webhooks::list_webhook_endpoints(
        Some(&app_name),
        query.include_inactive,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to list webhook endpoints via SDK: {:?}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list webhook endpoints")
    })?;
    
    // Convert JSON values to proper types
    let endpoints: Vec<WebhookEndpoint> = endpoints
        .into_iter()
        .filter_map(|e| serde_json::from_value(e).ok())
        .collect();
    
    Ok(ListWebhookEndpointsResponse {
        total: endpoints.len(),
        endpoints,
    }
    .into())
}

// Example: Get webhook deliveries using SDK
pub async fn get_webhook_deliveries(
    State(_app_state): State<HttpState>,
    ConsoleDeployment(_console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<DeliveryListQuery>,
) -> ApiResult<serde_json::Value> {
    let app_name = deployment_id.to_string();
    
    let deliveries = wacht::webhooks::get_webhook_deliveries(
        Some(&app_name),
        query.endpoint_id,
        query.event_name.as_deref(),
        query.status.as_deref(),
        query.limit,
        query.offset,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to get webhook deliveries via SDK: {:?}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get webhook deliveries")
    })?;
    
    Ok(serde_json::json!({
        "deliveries": deliveries,
        "total": deliveries.len()
    })
    .into())
}

// Example: Retry webhook delivery using SDK
pub async fn retry_webhook_delivery(
    State(_app_state): State<HttpState>,
    ConsoleDeployment(_console_deployment_id): ConsoleDeployment,
    RequireDeployment(_deployment_id): RequireDeployment,
    Path(delivery_id): Path<i64>,
) -> ApiResult<serde_json::Value> {
    let result = wacht::webhooks::retry_webhook_delivery(delivery_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to retry webhook delivery via SDK: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to retry webhook delivery")
        })?;
    
    Ok(result.into())
}

// Example: Reactivate webhook endpoint using SDK
pub async fn reactivate_webhook_endpoint(
    State(_app_state): State<HttpState>,
    ConsoleDeployment(_console_deployment_id): ConsoleDeployment,
    RequireDeployment(_deployment_id): RequireDeployment,
    Path(endpoint_id): Path<i64>,
) -> ApiResult<serde_json::Value> {
    let result = wacht::webhooks::reactivate_webhook_endpoint(endpoint_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to reactivate webhook endpoint via SDK: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to reactivate webhook endpoint")
        })?;
    
    Ok(result.into())
}

// Example: Get webhook analytics using SDK
pub async fn get_webhook_analytics(
    State(_app_state): State<HttpState>,
    ConsoleDeployment(_console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<ConsoleAnalyticsQuery>,
) -> ApiResult<serde_json::Value> {
    let app_name = deployment_id.to_string();
    
    let analytics = wacht::webhooks::get_webhook_analytics(
        Some(&app_name),
        query.start_date.as_deref(),
        query.end_date.as_deref(),
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to get webhook analytics via SDK: {:?}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get webhook analytics")
    })?;
    
    Ok(analytics.into())
}

// Note: Some console-specific endpoints like rotate_webhook_secret and get_available_events
// would still need custom implementation since they're specific to the console's use case