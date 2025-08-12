// Console-specific webhook management functions
// Webhook apps are stored in the console's database
// Each customer deployment can have one webhook app (named after their deployment_id)

use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;

use crate::application::{HttpState, response::ApiResult};
use crate::middleware::RequireDeployment;
use shared::{
    commands::{
        Command,
        webhook_app::{
            CreateWebhookAppCommand, RotateWebhookSecretCommand, UpdateWebhookAppCommand,
        },
        webhook_endpoint::{
            CreateWebhookEndpointCommand, DeleteWebhookEndpointCommand,
            UpdateWebhookEndpointCommand, EventSubscriptionData,
        },
        webhook_trigger::ReplayWebhookDeliveryCommand,
    },
    dto::json::{WebhookStatus, webhook_requests::*},
    models::webhook::{WebhookApp, WebhookEndpoint, WebhookEventDefinition},
    queries::{
        Query as QueryTrait,
        webhook::{GetWebhookAppByNameQuery, GetWebhookEndpointsQuery, GetWebhookStatsQuery},
        webhook_analytics::{GetWebhookAnalyticsQuery, GetWebhookTimeseriesQuery, TimeseriesInterval},
    },
};

// Helper function to get console deployment ID from environment
pub fn get_console_deployment_id() -> i64 {
    std::env::var("CONSOLE_DEPLOYMENT_ID")
        .expect("CONSOLE_DEPLOYMENT_ID environment variable must be set")
        .parse::<i64>()
        .expect("CONSOLE_DEPLOYMENT_ID must be a valid i64")
}

// Get webhook status for a deployment
pub async fn get_webhook_status(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<WebhookStatus> {
    let console_deployment_id = get_console_deployment_id();
    let app_name = deployment_id.to_string();
    
    let app = GetWebhookAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?;
    
    let stats = if let Some(ref app) = app {
        Some(GetWebhookStatsQuery::new(console_deployment_id, app.id)
            .execute(&app_state)
            .await?)
    } else {
        None
    };
    
    Ok(WebhookStatus {
        is_activated: app.is_some(),
        app,
        stats,
    }.into())
}

// Activate webhooks for a deployment
pub async fn activate_webhooks(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<WebhookApp> {
    let console_deployment_id = get_console_deployment_id();
    let app_name = deployment_id.to_string();
    
    // Check if already exists
    let existing = GetWebhookAppByNameQuery::new(console_deployment_id, app_name.clone())
        .execute(&app_state)
        .await?;
    
    if existing.is_some() {
        return Err((StatusCode::BAD_REQUEST, "Webhooks already activated for this deployment").into());
    }
    
    // Create webhook app in console's database
    let mut command = CreateWebhookAppCommand::new(console_deployment_id, app_name);
    command = command.with_description(format!("Platform events for deployment {}", deployment_id));
    
    // Define platform events that we send to customers
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
            name: "organization.created".to_string(),
            description: "New organization created".to_string(),
            schema: None,
        },
        WebhookEventDefinition {
            name: "workspace.created".to_string(),
            description: "New workspace created".to_string(),
            schema: None,
        },
    ];
    
    command = command.with_events(platform_events);
    
    let app = command.execute(&app_state).await?;
    Ok(app.into())
}

// Deactivate webhooks for a deployment
pub async fn deactivate_webhooks(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<()> {
    let console_deployment_id = get_console_deployment_id();
    let app_name = deployment_id.to_string();
    
    let app = GetWebhookAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Webhook app not found"))?;
    
    let command = UpdateWebhookAppCommand {
        app_id: app.id,
        deployment_id: console_deployment_id,
        name: None,
        description: None,
        is_active: Some(false),
        rate_limit_per_minute: None,
    };
    
    command.execute(&app_state).await?;
    Ok(().into())
}

// Rotate webhook signing secret
pub async fn rotate_webhook_secret(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<WebhookApp> {
    let console_deployment_id = get_console_deployment_id();
    let app_name = deployment_id.to_string();
    
    let app = GetWebhookAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Webhook app not found"))?;
    
    let command = RotateWebhookSecretCommand {
        app_id: app.id,
        deployment_id: console_deployment_id,
    };
    
    let app = command.execute(&app_state).await?;
    Ok(app.into())
}

// List webhook endpoints
pub async fn list_webhook_endpoints(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ListWebhookEndpointsQuery>,
) -> ApiResult<ListWebhookEndpointsResponse> {
    let console_deployment_id = get_console_deployment_id();
    let app_name = deployment_id.to_string();
    
    let app = GetWebhookAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Webhook app not found. Please activate webhooks first."))?;
    
    let include_inactive = params.include_inactive.unwrap_or(false);
    let endpoints = GetWebhookEndpointsQuery::new(console_deployment_id)
        .with_inactive(include_inactive)
        .for_app(app.id)
        .execute(&app_state)
        .await?;
    
    Ok(ListWebhookEndpointsResponse {
        total: endpoints.len(),
        endpoints,
    }.into())
}

// Create a webhook endpoint
pub async fn create_webhook_endpoint(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateWebhookEndpointRequest>,
) -> ApiResult<WebhookEndpoint> {
    let console_deployment_id = get_console_deployment_id();
    let app_name = deployment_id.to_string();
    
    let app = GetWebhookAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Webhook app not found. Please activate webhooks first."))?;
    
    let subscriptions: Vec<EventSubscriptionData> = request
        .subscriptions
        .into_iter()
        .map(|sub| EventSubscriptionData {
            event_name: sub.event_name,
            filter_rules: sub.filter_rules,
        })
        .collect();
    
    let command = CreateWebhookEndpointCommand {
        app_id: app.id,
        deployment_id: console_deployment_id,
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
    RequireDeployment(_deployment_id): RequireDeployment,
    Path(endpoint_id): Path<i64>,
    Json(request): Json<UpdateWebhookEndpointRequest>,
) -> ApiResult<WebhookEndpoint> {
    let console_deployment_id = get_console_deployment_id();
    
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
    RequireDeployment(_deployment_id): RequireDeployment,
    Path(endpoint_id): Path<i64>,
) -> ApiResult<()> {
    let console_deployment_id = get_console_deployment_id();
    
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
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ConsoleAnalyticsQuery>,
) -> ApiResult<serde_json::Value> {
    let console_deployment_id = get_console_deployment_id();
    let app_name = deployment_id.to_string();
    
    // Get the app to find its ID
    let app = GetWebhookAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?;
    
    if let Some(app) = app {
        let mut query = GetWebhookAnalyticsQuery::new(console_deployment_id)
            .with_app(app.id);
        
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
        }).into())
    }
}

pub async fn get_webhook_timeseries(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ConsoleTimeseriesQuery>,
) -> ApiResult<serde_json::Value> {
    let console_deployment_id = get_console_deployment_id();
    let app_name = deployment_id.to_string();
    
    // Get the app to find its ID
    let app = GetWebhookAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?;
    
    if let Some(app) = app {
        let interval = match params.interval.as_str() {
            "hour" => TimeseriesInterval::Hour,
            "day" => TimeseriesInterval::Day,
            _ => TimeseriesInterval::Hour,
        };
        
        let mut query = GetWebhookTimeseriesQuery::new(console_deployment_id, interval)
            .with_app(app.id);
        
        if let (Some(start), Some(end)) = (params.start_date, params.end_date) {
            query = query.with_date_range(start, end);
        }
        
        let result = query.execute(&app_state).await?;
        Ok(serde_json::to_value(result).unwrap().into())
    } else {
        Ok(serde_json::json!({
            "timeseries": []
        }).into())
    }
}

// Delivery history endpoints
pub async fn get_webhook_deliveries(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<DeliveryListQuery>,
) -> ApiResult<serde_json::Value> {
    let console_deployment_id = get_console_deployment_id();
    let app_name = deployment_id.to_string();
    
    // Get the app to find its ID
    let app = GetWebhookAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?;
    
    if let Some(app) = app {
        // Get recent deliveries from ClickHouse
        let limit = params.limit.unwrap_or(100) as usize;
        let deliveries = app_state.clickhouse_service
            .get_recent_webhook_deliveries(
                console_deployment_id,
                Some(app.id),
                params.status.as_deref(),
                params.event_name.as_deref(),
                limit,
            )
            .await?;
        
        Ok(serde_json::json!({
            "deliveries": deliveries
        }).into())
    } else {
        Ok(serde_json::json!({
            "deliveries": []
        }).into())
    }
}

pub async fn get_webhook_delivery_details(
    State(app_state): State<HttpState>,
    RequireDeployment(_deployment_id): RequireDeployment,
    Path(delivery_id): Path<i64>,
) -> ApiResult<serde_json::Value> {
    let console_deployment_id = get_console_deployment_id();
    
    // Get delivery details from ClickHouse
    let details = app_state.clickhouse_service
        .get_webhook_delivery_details(console_deployment_id, delivery_id)
        .await?;
    
    Ok(serde_json::to_value(details).unwrap().into())
}

pub async fn retry_webhook_delivery(
    State(app_state): State<HttpState>,
    RequireDeployment(_deployment_id): RequireDeployment,
    Path(delivery_id): Path<i64>,
) -> ApiResult<serde_json::Value> {
    let console_deployment_id = get_console_deployment_id();
    
    let new_delivery_id = ReplayWebhookDeliveryCommand {
        delivery_id,
        deployment_id: console_deployment_id,
    }
    .execute(&app_state)
    .await?;
    
    Ok(serde_json::json!({
        "new_delivery_id": new_delivery_id,
        "message": "Webhook delivery retried successfully"
    }).into())
}