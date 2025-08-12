use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{query, query_as};

use crate::{
    error::AppError,
    models::WebhookEndpoint,
    state::AppState,
};

use super::Command;

#[derive(Debug, Deserialize, Clone)]
pub struct EventSubscriptionData {
    pub event_name: String,
    pub filter_rules: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWebhookEndpointCommand {
    pub app_id: i64,
    pub deployment_id: i64,
    pub url: String,
    pub description: Option<String>,
    pub headers: Option<Value>,
    pub subscriptions: Vec<EventSubscriptionData>,
    pub max_retries: Option<i32>,
    pub timeout_seconds: Option<i32>,
}

impl CreateWebhookEndpointCommand {
    pub fn new(app_id: i64, deployment_id: i64, url: String) -> Self {
        Self {
            app_id,
            deployment_id,
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

        // Verify app exists and belongs to deployment
        let app_exists = query!(
            r#"
            SELECT 1 as exists
            FROM webhook_apps
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.app_id,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?
        .is_some();

        if !app_exists {
            return Err(AppError::NotFound("Webhook app not found".to_string()));
        }

        let mut tx = app_state.db_pool.begin().await?;

        // Create endpoint
        let headers_json = self.headers.unwrap_or_else(|| serde_json::json!({}));

        let endpoint = query_as!(
            WebhookEndpoint,
            r#"
            INSERT INTO webhook_endpoints (app_id, url, description, headers, max_retries, timeout_seconds, is_active)
            VALUES ($1, $2, $3, $4, $5, $6, true)
            RETURNING id as "id!", 
                      app_id as "app_id!", 
                      url as "url!", 
                      description, 
                      headers, 
                      is_active as "is_active!", 
                      max_retries as "max_retries!", 
                      timeout_seconds as "timeout_seconds!", 
                      created_at as "created_at!", 
                      updated_at as "updated_at!"
            "#,
            self.app_id,
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
                INSERT INTO webhook_endpoint_subscriptions (endpoint_id, event_id, filter_rules)
                SELECT $1, e.id, $3
                FROM webhook_app_events e
                WHERE e.app_id = $2 AND e.event_name = $4
                "#,
                endpoint.id,
                self.app_id,
                subscription.filter_rules,
                subscription.event_name
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
}

impl Command for UpdateWebhookEndpointCommand {
    type Output = WebhookEndpoint;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Validate URL if provided
        if let Some(ref url) = self.url {
            url::Url::parse(url)
                .map_err(|_| AppError::BadRequest("Invalid webhook URL".to_string()))?;
        }

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
            FROM webhook_apps a
            WHERE e.id = $1 
                AND e.app_id = a.id 
                AND a.deployment_id = $2
            RETURNING e.id as "id!", 
                      e.app_id as "app_id!", 
                      e.url as "url!", 
                      e.description, 
                      e.headers, 
                      e.is_active as "is_active!",
                      e.max_retries as "max_retries!", 
                      e.timeout_seconds as "timeout_seconds!", 
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
        .fetch_optional(&app_state.db_pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

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

        // Verify endpoint belongs to deployment
        let app_id = query!(
            r#"
            SELECT e.app_id
            FROM webhook_endpoints e
            JOIN webhook_apps a ON e.app_id = a.id
            WHERE e.id = $1 AND a.deployment_id = $2
            "#,
            self.endpoint_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?
        .app_id;

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
                INSERT INTO webhook_endpoint_subscriptions (endpoint_id, event_id, filter_rules)
                SELECT $1, e.id, $3
                FROM webhook_app_events e
                WHERE e.app_id = $2 AND e.event_name = $4
                "#,
                self.endpoint_id,
                app_id,
                self.filter_rules.clone(),
                event_name
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
            USING webhook_apps a
            WHERE e.id = $1 
                AND e.app_id = a.id 
                AND a.deployment_id = $2
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
        // Get endpoint details
        let endpoint = query!(
            r#"
            SELECT e.url, e.headers, e.timeout_seconds, a.signing_secret
            FROM webhook_endpoints e
            JOIN webhook_apps a ON e.app_id = a.id
            WHERE e.id = $1 AND a.deployment_id = $2
            "#,
            self.endpoint_id,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

        // Generate test signature
        let signature = crate::utils::webhook::generate_hmac_signature(
            &endpoint.signing_secret,
            &self.test_payload,
        );

        // Make test request
        let client = reqwest::Client::new();
        let mut request = client
            .post(&endpoint.url)
            .json(&self.test_payload)
            .header("X-Webhook-Signature", signature)
            .header("X-Webhook-Test", "true")
            .timeout(std::time::Duration::from_secs(endpoint.timeout_seconds.unwrap_or(30) as u64));

        // Add custom headers
        if let Some(headers_obj) = endpoint.headers.as_ref().and_then(|h| h.as_object()) {
            for (key, value) in headers_obj {
                if let Some(value_str) = value.as_str() {
                    request = request.header(key, value_str);
                }
            }
        }

        let start = std::time::Instant::now();
        let response = request.send().await;
        let duration = start.elapsed();

        match response {
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.ok();
                
                Ok(TestWebhookResult {
                    success: status.is_success(),
                    status_code: status.as_u16(),
                    response_time_ms: duration.as_millis() as u32,
                    response_body: body,
                    error: None,
                })
            }
            Err(e) => {
                Ok(TestWebhookResult {
                    success: false,
                    status_code: 0,
                    response_time_ms: duration.as_millis() as u32,
                    response_body: None,
                    error: Some(e.to_string()),
                })
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TestWebhookResult {
    pub success: bool,
    pub status_code: u16,
    pub response_time_ms: u32,
    pub response_body: Option<String>,
    pub error: Option<String>,
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
            FROM webhook_apps a
            WHERE e.id = $1 
                AND e.app_id = a.id 
                AND a.deployment_id = $2
            RETURNING e.id as "id!", 
                      e.app_id as "app_id!", 
                      e.url as "url!", 
                      e.description, 
                      e.headers, 
                      e.is_active as "is_active!",
                      e.max_retries as "max_retries!", 
                      e.timeout_seconds as "timeout_seconds!", 
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
        use super::webhook_delivery::ClearEndpointFailuresCommand;
        ClearEndpointFailuresCommand { 
            endpoint_id: self.endpoint_id 
        }
        .execute(app_state)
        .await?;

        Ok(endpoint)
    }
}