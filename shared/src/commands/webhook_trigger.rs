use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::query;
use chrono::Utc;

use crate::{
    error::AppError,
    services::clickhouse_webhook::{WebhookEvent, WebhookDelivery},
    state::AppState,
    utils::webhook::generate_hmac_signature,
};

use super::{Command, StoreWebhookPayloadCommand, GetSubscribedEndpointsCommand};
use super::webhook_subscription::evaluate_filter;

#[derive(Debug, Deserialize)]
pub struct TriggerWebhookEventCommand {
    pub deployment_id: i64,
    pub app_name: String,
    pub event_name: String,
    pub payload: Value,
    pub filter_context: Option<Value>,
}

impl TriggerWebhookEventCommand {
    pub fn new(deployment_id: i64, app_name: String, event_name: String, payload: Value) -> Self {
        Self {
            deployment_id,
            app_name,
            event_name,
            payload,
            filter_context: None,
        }
    }

    pub fn with_filter_context(mut self, context: Value) -> Self {
        self.filter_context = Some(context);
        self
    }

}

#[derive(Debug, Serialize)]
pub struct TriggerWebhookEventResult {
    pub delivery_ids: Vec<i64>,
    pub filtered_count: usize,
    pub delivered_count: usize,
}

impl Command for TriggerWebhookEventCommand {
    type Output = TriggerWebhookEventResult;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Get app info first
        let app_info = query!(
            r#"
            SELECT id, name
            FROM webhook_apps
            WHERE deployment_id = $1 AND name = $2 AND is_active = true
            "#,
            self.deployment_id,
            self.app_name
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        let app_info = match app_info {
            Some(app) => app,
            None => return Err(AppError::NotFound("Webhook app not found".to_string())),
        };

        // Log the webhook event to ClickHouse
        let event_id = app_state.sf.next_id().unwrap();
        let payload_size = self.payload.to_string().len() as i32;
        
        let event = WebhookEvent {
                deployment_id: self.deployment_id,
                app_id: app_info.id,
                app_name: app_info.name.clone(),
                event_name: self.event_name.clone(),
                event_id: event_id.to_string(),
                payload_size_bytes: payload_size,
                payload_s3_key: None,  // Will be set later if stored
                filter_context: self.filter_context.as_ref().map(|v| v.to_string()),
                timestamp: Utc::now(),
            };
            
        if let Err(e) = app_state.clickhouse_service.insert_webhook_event(&event).await {
            tracing::warn!("Failed to log webhook event to ClickHouse: {}", e);
        }

        // Get all subscribed endpoints using the cached command
        let endpoints = GetSubscribedEndpointsCommand::new(
            self.deployment_id,
            self.app_name.clone(),
            self.event_name.clone(),
        )
        .execute(app_state)
        .await?;

        let mut delivery_ids = Vec::new();
        let mut filtered_count = 0;

        for endpoint in endpoints {
            // Apply filter rules - evaluate against the event payload
            if let Some(ref filter_rules) = endpoint.filter_rules {
                if !evaluate_filter(filter_rules, &self.payload) {
                    filtered_count += 1;
                
                // Log filtered delivery to ClickHouse
                let delivery = WebhookDelivery {
                        deployment_id: self.deployment_id,
                        delivery_id: app_state.sf.next_id().unwrap() as i64,
                        app_id: app_info.id,
                        app_name: app_info.name.clone(),
                        endpoint_id: endpoint.id,
                        endpoint_url: endpoint.url.clone(),
                        event_name: self.event_name.clone(),
                        status: "filtered".to_string(),
                        http_status_code: None,
                        response_time_ms: None,
                        attempt_number: 0,
                        error_message: None,
                        filtered_reason: Some("Filter rules not matched".to_string()),
                        timestamp: Utc::now(),
                    };
                    
                if let Err(e) = app_state.clickhouse_service.insert_webhook_delivery(&delivery).await {
                    tracing::warn!("Failed to log filtered delivery to ClickHouse: {}", e);
                }
                
                tracing::debug!(
                    "Filtered out webhook delivery for endpoint {} due to filter rules",
                    endpoint.id
                );
                    continue;
                }
            }

            // Store payload in S3
            let s3_key = StoreWebhookPayloadCommand::new(self.payload.clone())
                .execute(app_state)
                .await?;

            // Generate HMAC signature
            let signature = generate_hmac_signature(&endpoint.signing_secret, &self.payload);

            // Queue for delivery
            let delivery = query!(
                r#"
                INSERT INTO active_webhook_deliveries 
                (endpoint_id, event_name, payload_s3_key, payload_size_bytes, signature, max_attempts) 
                VALUES ($1, $2, $3, $4, $5, $6) 
                RETURNING id
                "#,
                endpoint.id,
                self.event_name,
                s3_key,
                self.payload.to_string().len() as i32,
                signature,
                endpoint.max_retries
            )
            .fetch_one(&app_state.db_pool)
            .await?;

            delivery_ids.push(delivery.id);

            // Log pending delivery to ClickHouse
            let ch_delivery = WebhookDelivery {
                    deployment_id: self.deployment_id,
                    delivery_id: delivery.id,
                    app_id: app_info.id,
                    app_name: app_info.name.clone(),
                    endpoint_id: endpoint.id,
                    endpoint_url: endpoint.url.clone(),
                    event_name: self.event_name.clone(),
                    status: "pending".to_string(),
                    http_status_code: None,
                    response_time_ms: None,
                    attempt_number: 0,
                    error_message: None,
                    filtered_reason: None,
                    timestamp: Utc::now(),
                };
            
            if let Err(e) = app_state.clickhouse_service.insert_webhook_delivery(&ch_delivery).await {
                tracing::warn!("Failed to log pending delivery to ClickHouse: {}", e);
            }

            // Publish to NATS for async delivery via worker
            let task_message = serde_json::json!({
                "task_type": "webhook.deliver",
                "task_id": format!("webhook-{}-{}", delivery.id, self.deployment_id),
                "payload": {
                    "delivery_id": delivery.id,
                    "deployment_id": self.deployment_id
                }
            });

            app_state.nats_client
                .publish(
                    "worker.tasks.webhook.deliver",
                    serde_json::to_vec(&task_message)
                        .map_err(|e| AppError::Internal(format!("Failed to serialize task: {}", e)))?.into(),
                )
                .await
                .map_err(|e| AppError::Internal(format!("Failed to publish to NATS: {}", e)))?;
        }

        Ok(TriggerWebhookEventResult {
            delivered_count: delivery_ids.len(),
            delivery_ids,
            filtered_count,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct BatchTriggerWebhookEventsCommand {
    pub deployment_id: i64,
    pub app_name: String,
    pub events: Vec<WebhookEventTrigger>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookEventTrigger {
    pub event_name: String,
    pub payload: Value,
    pub filter_context: Option<Value>,
}

impl Command for BatchTriggerWebhookEventsCommand {
    type Output = Vec<TriggerWebhookEventResult>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut results = Vec::new();

        for event in self.events {
            let result = TriggerWebhookEventCommand::new(
                self.deployment_id,
                self.app_name.clone(),
                event.event_name,
                event.payload,
            )
            .with_filter_context(event.filter_context.unwrap_or(Value::Null))
            .execute(app_state)
            .await?;

            results.push(result);
        }

        Ok(results)
    }
}

#[derive(Debug, Deserialize)]
pub struct ReplayWebhookDeliveryCommand {
    pub delivery_id: i64,
    pub deployment_id: i64,
}

impl Command for ReplayWebhookDeliveryCommand {
    type Output = i64; // New delivery ID

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // First try to get from active deliveries
        let original = query!(
            r#"
            SELECT d.endpoint_id, d.event_name, d.payload_s3_key, d.payload_size_bytes, 
                   d.signature, d.max_attempts
            FROM active_webhook_deliveries d
            JOIN webhook_endpoints e ON d.endpoint_id = e.id
            JOIN webhook_apps a ON e.app_id = a.id
            WHERE d.id = $1 AND a.deployment_id = $2
            "#,
            self.delivery_id,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        let (endpoint_id, event_name, payload_s3_key, payload_size_bytes, signature, max_attempts) = 
            if let Some(o) = original {
                let eid = o.endpoint_id.ok_or_else(|| AppError::BadRequest("Delivery has no endpoint ID".to_string()))?;
                (eid, o.event_name, o.payload_s3_key, o.payload_size_bytes, o.signature, o.max_attempts)
            } else {
                // If not in active, we need to get the failed delivery info
                // For now, check if we have the S3 key stored somewhere
                return Err(AppError::NotFound(
                    "Delivery not found. Only active deliveries can be replayed currently.".to_string()
                ));
            };

        // Verify endpoint is active before replaying
        let endpoint_active = query!(
            r#"
            SELECT is_active
            FROM webhook_endpoints
            WHERE id = $1
            "#,
            endpoint_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

        if !endpoint_active.is_active.unwrap_or(false) {
            return Err(AppError::BadRequest(
                "Cannot replay delivery to inactive endpoint. Reactivate the endpoint first.".to_string()
            ));
        }

        // Create new delivery with reset attempts
        let new_delivery = query!(
            r#"
            INSERT INTO active_webhook_deliveries 
            (endpoint_id, event_name, payload_s3_key, payload_size_bytes, signature, max_attempts, attempts) 
            VALUES ($1, $2, $3, $4, $5, $6, 0) 
            RETURNING id
            "#,
            endpoint_id,
            event_name,
            payload_s3_key,
            payload_size_bytes,
            signature,
            max_attempts
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        // Log the replay to ClickHouse
        let ch_delivery = WebhookDelivery {
            deployment_id: self.deployment_id,
            delivery_id: new_delivery.id,
            app_id: 0, // We don't have this info here
            app_name: String::new(),
            endpoint_id,
            endpoint_url: String::new(),
            event_name: event_name.clone(),
            status: "replayed".to_string(),
            http_status_code: None,
            response_time_ms: None,
            attempt_number: 0,
            error_message: None,
            filtered_reason: None,
            timestamp: Utc::now(),
        };
        
        if let Err(e) = app_state.clickhouse_service.insert_webhook_delivery(&ch_delivery).await {
            tracing::warn!("Failed to log replay to ClickHouse: {}", e);
        }

        // Publish for immediate delivery via NATS
        let task_message = serde_json::json!({
            "task_type": "webhook.deliver",
            "task_id": format!("webhook-replay-{}", new_delivery.id),
            "priority": true,
            "payload": {
                "delivery_id": new_delivery.id,
                "deployment_id": self.deployment_id
            }
        });

        app_state.nats_client
            .publish(
                "worker.tasks.webhook.deliver",
                serde_json::to_vec(&task_message)?.into(),
            )
            .await
            .map_err(|e| AppError::Internal(format!("Failed to publish replay to NATS: {}", e)))?;

        Ok(new_delivery.id)
    }
}