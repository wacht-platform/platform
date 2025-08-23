// Console-specific webhook management functions
// These functions use the SDK to call backend API endpoints

use axum::extract::{Json, Path, Query};
use axum::http::StatusCode;
use wacht::api::webhooks;
use dto::json::{
    WebhookStatus,
    webhook_requests::{
        ConsoleAnalyticsQuery, ConsoleTimeseriesQuery, CreateWebhookEndpointConsoleRequest,
        DeliveryListQuery, GetAvailableEventsResponse, ListWebhookEndpointsQuery,
        ListWebhookEndpointsResponse, TestWebhookRequest, UpdateWebhookEndpointRequest,
        WebhookEndpointSubscription,
    },
};
use models::{
    webhook::{WebhookApp, WebhookEndpoint, WebhookEventDefinition},
};
use crate::application::response::ApiResult;
use crate::middleware::RequireDeployment;

// Get webhook status for a deployment
pub async fn get_webhook_status(
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<WebhookStatus> {
    let app_name = deployment_id.to_string();

    // Try to get app using SDK
    let app = match webhooks::get_webhook_app(&app_name).await {
        Ok(app_data) => {
            // Convert SDK response to model
            let app = WebhookApp {
                deployment_id: app_data.deployment_id,
                name: app_data.name,
                description: app_data.description,
                signing_secret: app_data.signing_secret,
                is_active: app_data.is_active,
                created_at: chrono::DateTime::parse_from_rfc3339(&app_data.created_at)
                    .unwrap_or_else(|_| chrono::Utc::now().into())
                    .with_timezone(&chrono::Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&app_data.updated_at)
                    .unwrap_or_else(|_| chrono::Utc::now().into())
                    .with_timezone(&chrono::Utc),
            };
            Some(app)
        }
        Err(_) => None,
    };

    let stats = if let Some(ref app) = app {
        match webhooks::get_webhook_stats(&app.name).await {
            Ok(stats_json) => {
                Some(dto::json::WebhookStats {
                    total_deliveries: stats_json.get("total_deliveries")
                        .and_then(|v| v.as_i64()).unwrap_or(0),
                    success_rate: stats_json.get("success_rate")
                        .and_then(|v| v.as_f64()).unwrap_or(0.0),
                    active_endpoints: stats_json.get("active_endpoints")
                        .and_then(|v| v.as_i64()).unwrap_or(0),
                    failed_deliveries_24h: stats_json.get("failed_deliveries_24h")
                        .and_then(|v| v.as_i64()).unwrap_or(0),
                })
            }
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
    ];

    // Convert model events to SDK events
    let sdk_events: Vec<webhooks::WebhookEventDefinition> = platform_events.iter().map(|e| {
        webhooks::WebhookEventDefinition {
            name: e.name.clone(),
            description: e.description.clone(),
            schema: e.schema.clone(),
        }
    }).collect();

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
        deployment_id: app_data.deployment_id,
        name: app_data.name,
        description: app_data.description,
        signing_secret: app_data.signing_secret,
        is_active: app_data.is_active,
        created_at: chrono::DateTime::parse_from_rfc3339(&app_data.created_at)
            .unwrap_or_else(|_| chrono::Utc::now().into())
            .with_timezone(&chrono::Utc),
        updated_at: chrono::DateTime::parse_from_rfc3339(&app_data.updated_at)
            .unwrap_or_else(|_| chrono::Utc::now().into())
            .with_timezone(&chrono::Utc),
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
        deployment_id: app_data.deployment_id,
        name: app_data.name,
        description: app_data.description,
        signing_secret: app_data.signing_secret,
        is_active: app_data.is_active,
        created_at: chrono::DateTime::parse_from_rfc3339(&app_data.created_at)
            .unwrap_or_else(|_| chrono::Utc::now().into())
            .with_timezone(&chrono::Utc),
        updated_at: chrono::DateTime::parse_from_rfc3339(&app_data.updated_at)
            .unwrap_or_else(|_| chrono::Utc::now().into())
            .with_timezone(&chrono::Utc),
    };
    
    Ok(app.into())
}

pub async fn list_webhook_endpoints(
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ListWebhookEndpointsQuery>,
) -> ApiResult<ListWebhookEndpointsResponse> {
    let app_name = deployment_id.to_string();

    // Check if app exists first
    webhooks::get_webhook_app(&app_name)
        .await
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                "Webhook app not found. Please activate webhooks first.",
            )
        })?;

    let include_inactive = params.include_inactive.unwrap_or(false);
    let endpoints_with_subs = webhooks::get_webhook_endpoints_with_subscriptions(&app_name, Some(include_inactive))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Convert to dto types
    let endpoints: Vec<dto::json::webhook_requests::WebhookEndpoint> = endpoints_with_subs.into_iter().map(|e| {
        // Convert subscribed events to subscription structs
        let subscriptions: Vec<WebhookEndpointSubscription> = e.subscribed_events.into_iter().map(|event_name| {
            WebhookEndpointSubscription {
                event_name,
                filter_rules: None, // These will be populated if needed
            }
        }).collect();
        
        dto::json::webhook_requests::WebhookEndpoint {
            id: e.endpoint.id,
            deployment_id: e.endpoint.deployment_id,
            app_name: e.endpoint.app_name,
            url: e.endpoint.url,
            description: e.endpoint.description,
            headers: e.endpoint.headers,
            is_active: e.endpoint.is_active,
            signing_secret: e.endpoint.signing_secret,
            max_retries: e.endpoint.max_retries,
            timeout_seconds: e.endpoint.timeout_seconds,
            failure_count: e.endpoint.failure_count,
            last_failure_at: e.endpoint.last_failure_at.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            auto_disabled: e.endpoint.auto_disabled,
            auto_disabled_at: e.endpoint.auto_disabled_at.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            created_at: chrono::DateTime::parse_from_rfc3339(&e.endpoint.created_at)
                .unwrap_or_else(|_| chrono::Utc::now().into())
                .with_timezone(&chrono::Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339(&e.endpoint.updated_at)
                .unwrap_or_else(|_| chrono::Utc::now().into())
                .with_timezone(&chrono::Utc),
            subscriptions,
        }
    }).collect();

    Ok(ListWebhookEndpointsResponse {
        total: endpoints.len(),
        endpoints,
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
    webhooks::get_webhook_app(&app_name)
        .await
        .map_err(|_| {
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
        id: endpoint_data.id,
        deployment_id: endpoint_data.deployment_id,
        app_name: endpoint_data.app_name,
        url: endpoint_data.url,
        description: endpoint_data.description,
        headers: endpoint_data.headers,
        is_active: endpoint_data.is_active,
        signing_secret: endpoint_data.signing_secret,
        max_retries: endpoint_data.max_retries,
        timeout_seconds: endpoint_data.timeout_seconds,
        failure_count: endpoint_data.failure_count,
        last_failure_at: endpoint_data.last_failure_at.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc)),
        auto_disabled: endpoint_data.auto_disabled,
        auto_disabled_at: endpoint_data.auto_disabled_at.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc)),
        created_at: chrono::DateTime::parse_from_rfc3339(&endpoint_data.created_at)
            .unwrap_or_else(|_| chrono::Utc::now().into())
            .with_timezone(&chrono::Utc),
        updated_at: chrono::DateTime::parse_from_rfc3339(&endpoint_data.updated_at)
            .unwrap_or_else(|_| chrono::Utc::now().into())
            .with_timezone(&chrono::Utc),
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
    };

    let endpoint_data = webhooks::update_webhook_endpoint(endpoint_id, sdk_request)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    let endpoint = WebhookEndpoint {
        id: endpoint_data.id,
        deployment_id: endpoint_data.deployment_id,
        app_name: endpoint_data.app_name,
        url: endpoint_data.url,
        description: endpoint_data.description,
        headers: endpoint_data.headers,
        is_active: endpoint_data.is_active,
        signing_secret: endpoint_data.signing_secret,
        max_retries: endpoint_data.max_retries,
        timeout_seconds: endpoint_data.timeout_seconds,
        failure_count: endpoint_data.failure_count,
        last_failure_at: endpoint_data.last_failure_at.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc)),
        auto_disabled: endpoint_data.auto_disabled,
        auto_disabled_at: endpoint_data.auto_disabled_at.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc)),
        created_at: chrono::DateTime::parse_from_rfc3339(&endpoint_data.created_at)
            .unwrap_or_else(|_| chrono::Utc::now().into())
            .with_timezone(&chrono::Utc),
        updated_at: chrono::DateTime::parse_from_rfc3339(&endpoint_data.updated_at)
            .unwrap_or_else(|_| chrono::Utc::now().into())
            .with_timezone(&chrono::Utc),
    };
    
    Ok(endpoint.into())
}

// Delete a webhook endpoint
pub async fn delete_webhook_endpoint(
    Path((_deployment_id_path, endpoint_id)): Path<(i64, i64)>,
) -> ApiResult<()> {
    webhooks::delete_webhook_endpoint(endpoint_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    Ok(().into())
}

// Analytics endpoints
pub async fn get_webhook_analytics(
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ConsoleAnalyticsQuery>,
) -> ApiResult<serde_json::Value> {
    let app_name = deployment_id.to_string();

    // Check if app exists
    let app_exists = webhooks::get_webhook_app(&app_name).await.is_ok();

    if app_exists {
        let start_date = params.start_date.map(|d| d.to_rfc3339());
        let end_date = params.end_date.map(|d| d.to_rfc3339());
        
        let result = webhooks::get_webhook_analytics(
            Some(&app_name), 
            start_date.as_deref(), 
            end_date.as_deref()
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        
        Ok(result.into())
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
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ConsoleTimeseriesQuery>,
) -> ApiResult<serde_json::Value> {
    let app_name = deployment_id.to_string();

    eprintln!(
        "get_webhook_timeseries: deployment_id={}, app_name={}",
        deployment_id, app_name
    );
    eprintln!(
        "Query params: start={:?}, end={:?}, interval={}",
        params.start_date, params.end_date, params.interval
    );

    // Check if app exists
    let app_exists = match webhooks::get_webhook_app(&app_name).await {
        Ok(_) => true,
        Err(e) => {
            eprintln!("Failed to get webhook app: {:?}", e);
            false
        }
    };

    if app_exists {
        eprintln!("Found app: name={}", app_name);

        let start_date = params.start_date.map(|d| d.to_rfc3339());
        let end_date = params.end_date.map(|d| d.to_rfc3339());

        eprintln!("Executing timeseries query with app_name={}", app_name);

        match webhooks::get_webhook_timeseries(
            &app_name,
            &params.interval,
            start_date.as_deref(),
            end_date.as_deref()
        ).await {
            Ok(result) => {
                eprintln!(
                    "Timeseries query successful"
                );
                Ok(result.into())
            }
            Err(e) => {
                eprintln!("Timeseries query failed: {:?}", e);
                Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into())
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
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<DeliveryListQuery>,
) -> ApiResult<serde_json::Value> {
    let app_name = deployment_id.to_string();

    // Check if app exists
    let app_exists = webhooks::get_webhook_app(&app_name).await.is_ok();

    if app_exists {
        let deliveries = webhooks::get_webhook_deliveries(
            Some(&app_name),
            None, // endpoint_id
            params.event_name.as_deref(),
            params.status.as_deref(),
            params.limit.map(|l| l as i32),
            params.offset.map(|o| o as i32),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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
    Path((_deployment_id_path, delivery_id)): Path<(i64, i64)>,
) -> ApiResult<serde_json::Value> {
    let details = webhooks::get_webhook_delivery_details(delivery_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(details.into())
}

pub async fn retry_webhook_delivery(
    Path((_deployment_id_path, delivery_id)): Path<(i64, i64)>,
) -> ApiResult<serde_json::Value> {
    let result = webhooks::retry_webhook_delivery(delivery_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    Ok(result.into())
}

// Test webhook endpoint
pub async fn test_webhook_endpoint(
    Path((_, endpoint_id)): Path<(i64, i64)>,
    Json(request): Json<TestWebhookRequest>,
) -> ApiResult<serde_json::Value> {
    let result = webhooks::test_webhook_endpoint(
        endpoint_id,
        request.event_name.clone(),
        request.payload,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(result.into())
}

// Reactivate webhook endpoint
pub async fn reactivate_webhook_endpoint(
    Path((_deployment_id_path, endpoint_id)): Path<(i64, i64)>,
) -> ApiResult<serde_json::Value> {
    let result = webhooks::reactivate_webhook_endpoint(endpoint_id)
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
    webhooks::get_webhook_app(&app_name)
        .await
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                "Webhook app not found. Please activate webhooks first.",
            )
        })?;

    // Get events for this app
    let events_data = webhooks::get_webhook_events(&app_name)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Convert to model types
    let events: Vec<models::webhook::WebhookAppEvent> = events_data.into_iter().map(|e| {
        models::webhook::WebhookAppEvent {
            deployment_id: 0, // This will be filled by backend
            app_name: app_name.clone(),
            event_name: e.name,
            description: Some(e.description),
            schema: e.schema,
            created_at: chrono::Utc::now(),
        }
    }).collect();

    Ok(GetAvailableEventsResponse { events }.into())
}