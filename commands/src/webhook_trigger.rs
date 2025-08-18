use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::query;

use common::error::AppError;
use common::state::AppState;
use common::utils::webhook::generate_hmac_signature;
use dto::clickhouse::webhook::{WebhookDelivery, WebhookEvent};
use dto::json::nats::NatsTaskMessage;
use models::webhook::WebhookEventTrigger;

use super::{Command, GetSubscribedEndpointsCommand};
use super::webhook_subscription::evaluate_filter;
use super::webhook_storage::{StoreWebhookPayloadCommand, RetrieveWebhookPayloadCommand};

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
            SELECT name
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
                app_name: app_info.name.clone(),
                event_name: self.event_name.clone(),
                event_id: event_id.to_string(),
                payload_size_bytes: payload_size,
                filter_context: self.filter_context.as_ref().map(|v| v.to_string()),
                timestamp: Utc::now(),
            };
            
        if let Err(e) = app_state.clickhouse_service.insert_webhook_event(&event).await {
            tracing::warn!("Failed to log webhook event to ClickHouse: {}", e);
        }

        // Store payload in S3 once for all deliveries
        let payload_s3_key = StoreWebhookPayloadCommand::new(self.payload.clone())
            .execute(app_state)
            .await?;

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
                        app_name: app_info.name.clone(),
                        endpoint_id: endpoint.id,
                        endpoint_url: endpoint.url.clone(),
                        event_name: self.event_name.clone(),
                        status: "filtered".to_string(),
                        http_status_code: None,
                        response_time_ms: None,
                        attempt_number: 0,
                        max_attempts: endpoint.max_retries,
                        error_message: None,
                        filtered_reason: Some("Filter rules not matched".to_string()),
                        payload_s3_key: payload_s3_key.clone(),
                        response_body: None,
                        response_headers: None,
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

            // Generate HMAC signature
            let signature = generate_hmac_signature(&endpoint.signing_secret, &self.payload);

            // Generate Snowflake ID for delivery
            let delivery_id = app_state.sf.next_id()? as i64;

            // Queue for delivery
            let delivery = query!(
                r#"
                INSERT INTO active_webhook_deliveries 
                (id, endpoint_id, deployment_id, app_name, event_name, payload_s3_key, payload_size_bytes, signature, max_attempts) 
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) 
                RETURNING id
                "#,
                delivery_id,
                endpoint.id,
                self.deployment_id,
                app_info.name.clone(),
                self.event_name,
                payload_s3_key,
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
                    app_name: app_info.name.clone(),
                    endpoint_id: endpoint.id,
                    endpoint_url: endpoint.url.clone(),
                    event_name: self.event_name.clone(),
                    status: "pending".to_string(),
                    http_status_code: None,
                    response_time_ms: None,
                    attempt_number: 0,
                    max_attempts: endpoint.max_retries,
                    error_message: None,
                    filtered_reason: None,
                    payload_s3_key: payload_s3_key.clone(),
                    response_body: None,
                    response_headers: None,
                    timestamp: Utc::now(),
                };
            
            if let Err(e) = app_state.clickhouse_service.insert_webhook_delivery(&ch_delivery).await {
                tracing::warn!("Failed to log pending delivery to ClickHouse: {}", e);
            }

            // Publish to NATS for async delivery via worker
            let task_message = NatsTaskMessage {
                task_type: "webhook.deliver".to_string(),
                task_id: format!("webhook-{}-{}", delivery.id, self.deployment_id),
                payload: serde_json::json!({
                    "delivery_id": delivery.id,
                    "deployment_id": self.deployment_id
                }),
                retry_count: 0,
                max_retries: 3,
            };

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
        tracing::info!("Starting replay for delivery_id: {}, deployment_id: {}", self.delivery_id, self.deployment_id);
        
        // Check if delivery is still active - we don't allow replaying active deliveries
        let is_active = query!(
            r#"
            SELECT 1 as exists
            FROM active_webhook_deliveries
            WHERE id = $1 AND deployment_id = $2
            LIMIT 1
            "#,
            self.delivery_id,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;
        
        if is_active.is_some() {
            return Err(AppError::BadRequest(
                "Cannot replay an active delivery. Please wait for it to complete or cancel it first.".to_string()
            ));
        }
        
        // Get delivery details from ClickHouse (only completed deliveries)
        let (endpoint_id, event_name, payload_s3_key, payload_size_bytes, signature, max_attempts, app_name, endpoint_url): (i64, String, String, i32, Option<String>, i32, String, String) = {
            let delivery_details = app_state.clickhouse_service
                .get_webhook_delivery_details(self.deployment_id, self.delivery_id)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to get delivery details from ClickHouse: {}", e);
                    AppError::NotFound("Delivery not found or not yet completed.".to_string())
                })?;
            
            tracing::debug!("Got delivery details from ClickHouse: {:?}", delivery_details);
            
            // Extract the necessary fields from the ClickHouse data
            let endpoint_id = if let Some(id_str) = delivery_details["endpoint_id"].as_str() {
                id_str.parse::<i64>()
                    .map_err(|e| AppError::BadRequest(format!("Invalid endpoint ID string: {}", e)))?
            } else if let Some(id_num) = delivery_details["endpoint_id"].as_i64() {
                id_num
            } else {
                return Err(AppError::BadRequest("Missing or invalid endpoint ID in delivery details".to_string()));
            };
            
            let event_name = delivery_details["event_name"]
                .as_str()
                .ok_or_else(|| AppError::BadRequest("Missing event name in delivery details".to_string()))?
                .to_string();
            
            let original_s3_key = delivery_details["payload_s3_key"]
                .as_str()
                .ok_or_else(|| AppError::BadRequest("Cannot replay this delivery. The original payload is no longer available.".to_string()))?;
            
            // Retrieve the original payload from S3
            let payload = RetrieveWebhookPayloadCommand::new(original_s3_key.to_string())
                .execute(app_state)
                .await
                .map_err(|e| AppError::BadRequest(format!("Failed to retrieve original payload: {}", e)))?;
            
            // Store the payload in S3 for the new delivery
            let payload_s3_key = StoreWebhookPayloadCommand::new(payload.clone())
                .execute(app_state)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to store webhook payload for retry: {}", e);
                    e
                })?;
            
            // Get endpoint details to generate new signature
            tracing::info!("Looking up endpoint with id: {} for retry", endpoint_id);
            let endpoint = query!(
                r#"
                SELECT e.id, e.url, e.max_retries, e.app_name, a.signing_secret, e.deployment_id
                FROM webhook_endpoints e
                JOIN webhook_apps a ON (e.deployment_id = a.deployment_id AND e.app_name = a.name)
                WHERE e.id = $1
                "#,
                endpoint_id
            )
            .fetch_optional(&app_state.db_pool)
            .await?;
            
            if endpoint.is_none() {
                tracing::warn!("Endpoint {} no longer exists - it may have been deleted", endpoint_id);
                return Err(AppError::NotFound(
                    "Cannot retry delivery: The webhook endpoint has been deleted. Please create a new endpoint and trigger the event again.".to_string()
                ));
            }
            
            let endpoint = endpoint.unwrap();
            
            // Verify deployment_id matches
            if endpoint.deployment_id != self.deployment_id {
                tracing::error!("Deployment mismatch: endpoint belongs to deployment {}, but retry requested for deployment {}", 
                    endpoint.deployment_id, self.deployment_id);
                return Err(AppError::BadRequest(format!(
                    "Endpoint belongs to different deployment"
                )));
            }
            
            // Generate new HMAC signature
            let signature = Some(generate_hmac_signature(&endpoint.signing_secret, &payload));
            let payload_size_bytes = serde_json::to_string(&payload).unwrap_or_default().len() as i32;
            let max_attempts = endpoint.max_retries.unwrap_or(5);
            let app_name = endpoint.app_name;
            let endpoint_url = endpoint.url;
            
            (endpoint_id, event_name, payload_s3_key, payload_size_bytes, signature, max_attempts, app_name, endpoint_url)
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

        // Generate Snowflake ID for new delivery
        let new_delivery_id = app_state.sf.next_id()? as i64;
        
        // Create new delivery with reset attempts
        let new_delivery = query!(
            r#"
            INSERT INTO active_webhook_deliveries 
            (id, endpoint_id, deployment_id, app_name, event_name, payload_s3_key, payload_size_bytes, signature, max_attempts, attempts) 
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 0) 
            RETURNING id
            "#,
            new_delivery_id,
            endpoint_id,
            self.deployment_id,
            app_name.clone(),
            event_name.clone(),
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
            app_name: app_name.clone(),
            endpoint_id,
            endpoint_url: endpoint_url.clone(),
            event_name: event_name.clone(),
            status: "replayed".to_string(),
            http_status_code: None,
            response_time_ms: None,
            attempt_number: 0,
            max_attempts,
            error_message: None,
            filtered_reason: None,
            payload_s3_key: payload_s3_key.clone(),
            response_body: None,
            response_headers: None,
            timestamp: Utc::now(),
        };
        
        if let Err(e) = app_state.clickhouse_service.insert_webhook_delivery(&ch_delivery).await {
            tracing::warn!("Failed to log replay to ClickHouse: {}", e);
        }

        // Publish for immediate delivery via NATS
        let task_message = NatsTaskMessage {
            task_type: "webhook.deliver".to_string(),
            task_id: format!("webhook-replay-{}", new_delivery.id),
            payload: serde_json::json!({
                "delivery_id": new_delivery.id,
                "deployment_id": self.deployment_id
            }),
            retry_count: 0,
            max_retries: 3,
        };

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