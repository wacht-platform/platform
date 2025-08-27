use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{query, query_as};

use crate::Command;
use crate::webhook_delivery::ClearEndpointFailuresCommand;
use crate::webhook_storage::StoreWebhookPayloadCommand;
use common::error::AppError;
use common::state::AppState;
use common::utils::webhook::generate_hmac_signature;
use dto::clickhouse::webhook::WebhookEvent;
use dto::json::nats::NatsTaskMessage;
use models::WebhookEndpoint;

#[derive(Debug, Deserialize, Clone)]
pub struct EventSubscriptionData {
    pub event_name: String,
    pub filter_rules: Option<Value>,
}

impl From<dto::json::webhook_requests::EventSubscription> for EventSubscriptionData {
    fn from(s: dto::json::webhook_requests::EventSubscription) -> Self {
        Self {
            event_name: s.event_name,
            filter_rules: s.filter_rules,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateWebhookEndpointCommand {
    pub deployment_id: i64,
    pub app_name: String,
    pub url: String,
    pub description: Option<String>,
    pub headers: Option<Value>,
    pub subscriptions: Vec<EventSubscriptionData>,
    pub max_retries: Option<i32>,
    pub timeout_seconds: Option<i32>,
}

impl CreateWebhookEndpointCommand {
    pub fn new(deployment_id: i64, app_name: String, url: String) -> Self {
        Self {
            deployment_id,
            app_name,
            url,
            description: None,
            headers: None,
            subscriptions: Vec::new(),
            max_retries: None,
            timeout_seconds: None,
        }
    }

    pub fn with_subscriptions(mut self, subscriptions: Vec<EventSubscriptionData>) -> Self {
        self.subscriptions = subscriptions;
        self
    }

    pub fn with_headers(mut self, headers: Value) -> Self {
        self.headers = Some(headers);
        self
    }
}

impl Command for CreateWebhookEndpointCommand {
    type Output = WebhookEndpoint;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Validate URL
        url::Url::parse(&self.url)
            .map_err(|_| AppError::BadRequest("Invalid webhook URL".to_string()))?;

        // Verify app exists
        let app_exists = query!(
            r#"
            SELECT 1 as exists
            FROM webhook_apps
            WHERE deployment_id = $1 AND name = $2
            "#,
            self.deployment_id,
            self.app_name
        )
        .fetch_optional(&app_state.db_pool)
        .await?
        .is_some();

        if !app_exists {
            return Err(AppError::NotFound("Webhook app not found".to_string()));
        }

        let mut tx = app_state.db_pool.begin().await?;

        // Create endpoint with Snowflake ID
        let endpoint_id = app_state.sf.next_id()? as i64;
        let headers_json = self.headers.unwrap_or_else(|| serde_json::json!({}));

        let endpoint = query_as!(
            WebhookEndpoint,
            r#"
            INSERT INTO webhook_endpoints (id, deployment_id, app_name, url, description, headers, max_retries, timeout_seconds, is_active)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, true)
            RETURNING id as "id!",
                      deployment_id as "deployment_id!",
                      app_name as "app_name!",
                      url as "url!",
                      description,
                      headers,
                      signing_secret,
                      is_active as "is_active!",
                      max_retries as "max_retries!",
                      timeout_seconds as "timeout_seconds!",
                      failure_count as "failure_count!",
                      last_failure_at,
                      auto_disabled as "auto_disabled!",
                      auto_disabled_at,
                      created_at as "created_at!",
                      updated_at as "updated_at!"
            "#,
            endpoint_id,
            self.deployment_id,
            self.app_name,
            self.url,
            self.description,
            headers_json,
            self.max_retries.unwrap_or(5) as i32,
            self.timeout_seconds.unwrap_or(30) as i32
        )
        .fetch_one(&mut *tx)
        .await?;

        // Subscribe to events with individual filter rules
        for subscription in self.subscriptions {
            query!(
                r#"
                INSERT INTO webhook_endpoint_subscriptions (endpoint_id, deployment_id, app_name, event_name, filter_rules)
                VALUES ($1, $2, $3, $4, $5)
                "#,
                endpoint.id,
                self.deployment_id,
                self.app_name,
                subscription.event_name,
                subscription.filter_rules
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(endpoint)
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateWebhookEndpointCommand {
    pub endpoint_id: i64,
    pub deployment_id: i64,
    pub url: Option<String>,
    pub description: Option<String>,
    pub headers: Option<Value>,
    pub is_active: Option<bool>,
    pub max_retries: Option<i32>,
    pub timeout_seconds: Option<i32>,
    pub subscriptions: Option<Vec<EventSubscriptionData>>,
}

impl Command for UpdateWebhookEndpointCommand {
    type Output = WebhookEndpoint;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Validate URL if provided
        if let Some(ref url) = self.url {
            url::Url::parse(url)
                .map_err(|_| AppError::BadRequest("Invalid webhook URL".to_string()))?;
        }

        let mut tx = app_state.db_pool.begin().await?;

        let endpoint = query_as!(
            WebhookEndpoint,
            r#"
            UPDATE webhook_endpoints e
            SET url = COALESCE($3, e.url),
                description = COALESCE($4, e.description),
                headers = COALESCE($5, e.headers),
                is_active = COALESCE($6, e.is_active),
                max_retries = COALESCE($7, e.max_retries),
                timeout_seconds = COALESCE($8, e.timeout_seconds)
            WHERE e.id = $1 AND e.deployment_id = $2
            RETURNING e.id as "id!",
                      e.deployment_id as "deployment_id!",
                      e.app_name as "app_name!",
                      e.url as "url!",
                      e.description,
                      e.headers,
                      e.signing_secret,
                      e.is_active as "is_active!",
                      e.max_retries as "max_retries!",
                      e.timeout_seconds as "timeout_seconds!",
                      e.failure_count as "failure_count!",
                      e.last_failure_at,
                      e.auto_disabled as "auto_disabled!",
                      e.auto_disabled_at,
                      e.created_at as "created_at!",
                      e.updated_at as "updated_at!"
            "#,
            self.endpoint_id,
            self.deployment_id,
            self.url,
            self.description,
            self.headers,
            self.is_active,
            self.max_retries,
            self.timeout_seconds
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

        // Update subscriptions if provided
        if let Some(subscriptions) = self.subscriptions {
            // Clear existing subscriptions
            query!(
                r#"
                DELETE FROM webhook_endpoint_subscriptions
                WHERE endpoint_id = $1
                "#,
                self.endpoint_id
            )
            .execute(&mut *tx)
            .await?;

            // Add new subscriptions with individual filter rules
            for subscription in subscriptions {
                query!(
                    r#"
                    INSERT INTO webhook_endpoint_subscriptions (endpoint_id, deployment_id, app_name, event_name, filter_rules)
                    VALUES ($1, $2, $3, $4, $5)
                    "#,
                    self.endpoint_id,
                    self.deployment_id,
                    endpoint.app_name.clone(),
                    subscription.event_name,
                    subscription.filter_rules
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        Ok(endpoint)
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateEndpointSubscriptionsCommand {
    pub endpoint_id: i64,
    pub deployment_id: i64,
    pub subscribe_to_events: Vec<String>,
    pub filter_rules: Option<Value>,
}

impl Command for UpdateEndpointSubscriptionsCommand {
    type Output = Vec<String>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut tx = app_state.db_pool.begin().await?;

        // Verify endpoint belongs to deployment and get app_name
        let app_name = query!(
            r#"
            SELECT e.app_name
            FROM webhook_endpoints e
            WHERE e.id = $1 AND e.deployment_id = $2
            "#,
            self.endpoint_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?
        .app_name;

        // Clear existing subscriptions
        query!(
            r#"
            DELETE FROM webhook_endpoint_subscriptions
            WHERE endpoint_id = $1
            "#,
            self.endpoint_id
        )
        .execute(&mut *tx)
        .await?;

        // Add new subscriptions
        for event_name in &self.subscribe_to_events {
            query!(
                r#"
                INSERT INTO webhook_endpoint_subscriptions (endpoint_id, deployment_id, app_name, event_name, filter_rules)
                VALUES ($1, $2, $3, $4, $5)
                "#,
                self.endpoint_id,
                self.deployment_id,
                app_name.clone(),
                event_name,
                self.filter_rules.clone()
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(self.subscribe_to_events)
    }
}

#[derive(Debug, Deserialize)]
pub struct DeleteWebhookEndpointCommand {
    pub endpoint_id: i64,
    pub deployment_id: i64,
}

impl Command for DeleteWebhookEndpointCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = query!(
            r#"
            DELETE FROM webhook_endpoints e
            WHERE e.id = $1 AND e.deployment_id = $2
            "#,
            self.endpoint_id,
            self.deployment_id
        )
        .execute(&app_state.db_pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Webhook endpoint not found".to_string()));
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct TestWebhookEndpointCommand {
    pub endpoint_id: i64,
    pub deployment_id: i64,
    pub test_payload: Value,
}

impl Command for TestWebhookEndpointCommand {
    type Output = TestWebhookResult;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Get endpoint details to validate it exists
        let endpoint = query!(
            r#"
            SELECT e.url, e.headers, e.timeout_seconds, e.app_name, a.signing_secret
            FROM webhook_endpoints e
            JOIN webhook_apps a ON (e.deployment_id = a.deployment_id AND e.app_name = a.name)
            WHERE e.id = $1 AND e.deployment_id = $2
            "#,
            self.endpoint_id,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

        // App name is already in endpoint
        let app_name = endpoint.app_name.clone();

        // Record test delivery to ClickHouse for analytics

        let delivery_id = app_state.sf.next_id()? as i64;
        let event_id = app_state.sf.next_id()?.to_string();
        let now = Utc::now();

        // Store test payload in S3
        let payload_s3_key = StoreWebhookPayloadCommand::new(self.test_payload.clone())
            .execute(app_state)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to store test payload: {}", e)))?;

        let payload_size = serde_json::to_string(&self.test_payload)
            .unwrap_or_default()
            .len() as i32;

        // Log the event to ClickHouse
        let ch_event = WebhookEvent {
            deployment_id: self.deployment_id,
            app_name: app_name.clone(),
            event_id,
            event_name: "test.webhook".to_string(),
            payload_size_bytes: payload_size,
            filter_context: None,
            timestamp: now,
        };

        if let Err(e) = app_state
            .clickhouse_service
            .insert_webhook_event(&ch_event)
            .await
        {
            tracing::warn!("Failed to log test event to ClickHouse: {}", e);
        }

        // Create delivery in active_webhook_deliveries and let worker process it
        let signature = generate_hmac_signature(&endpoint.signing_secret, &self.test_payload);

        query!(
            r#"
            INSERT INTO active_webhook_deliveries
            (id, endpoint_id, deployment_id, app_name, event_name, payload_s3_key, payload_size_bytes, signature, max_attempts, attempts)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 1, 0)
            "#,
            delivery_id,
            self.endpoint_id,
            self.deployment_id,
            app_name,
            "test.webhook",
            payload_s3_key,
            payload_size,
            signature
        )
        .execute(&app_state.db_pool)
        .await?;

        // Publish to worker for immediate processing
        let task_message = NatsTaskMessage {
            task_type: "webhook.deliver".to_string(),
            task_id: format!("webhook-test-{}", delivery_id),
            payload: serde_json::json!({
                "delivery_id": delivery_id,
                "deployment_id": self.deployment_id
            }),
        };

        app_state
            .nats_client
            .publish(
                "worker.tasks.webhook.deliver",
                serde_json::to_vec(&task_message)?.into(),
            )
            .await
            .map_err(|e| AppError::Internal(format!("Failed to publish test webhook: {}", e)))?;

        // Return simple success response - actual results will be available in delivery history
        Ok(TestWebhookResult {
            delivery_id: Some(delivery_id),
            success: true,
            status_code: 202, // Accepted
            response_time_ms: 0,
            response_body: Some(
                "Test webhook initiated. Check delivery history for results.".to_string(),
            ),
            response_content_type: Some("text/plain".to_string()),
            error: None,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct TestWebhookResult {
    pub success: bool,
    pub status_code: u16,
    pub response_time_ms: u32,
    pub response_body: Option<String>,
    pub response_content_type: Option<String>,
    pub error: Option<String>,
    pub delivery_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ReactivateEndpointCommand {
    pub endpoint_id: i64,
    pub deployment_id: i64,
}

impl Command for ReactivateEndpointCommand {
    type Output = WebhookEndpoint;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Verify endpoint belongs to deployment and reactivate in one query
        let endpoint = query_as!(
            WebhookEndpoint,
            r#"
            UPDATE webhook_endpoints e
            SET is_active = true, updated_at = NOW()
            WHERE e.id = $1 AND e.deployment_id = $2
            RETURNING e.id as "id!",
                      e.deployment_id as "deployment_id!",
                      e.app_name as "app_name!",
                      e.url as "url!",
                      e.description,
                      e.headers,
                      e.signing_secret,
                      e.is_active as "is_active!",
                      e.max_retries as "max_retries!",
                      e.timeout_seconds as "timeout_seconds!",
                      e.failure_count as "failure_count!",
                      e.last_failure_at,
                      e.auto_disabled as "auto_disabled!",
                      e.auto_disabled_at,
                      e.created_at as "created_at!",
                      e.updated_at as "updated_at!"
            "#,
            self.endpoint_id,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

        // Clear failure counter in Redis
        ClearEndpointFailuresCommand {
            endpoint_id: self.endpoint_id,
        }
        .execute(app_state)
        .await?;

        Ok(endpoint)
    }
}
