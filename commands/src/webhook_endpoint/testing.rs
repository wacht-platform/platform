use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{query, query_as};

use crate::webhook_delivery::ClearEndpointFailuresCommand;
use common::capabilities::HasRedis;
use common::error::AppError;
use common::utils::webhook::generate_webhook_signature;
use dto::clickhouse::webhook::WebhookLog;
use dto::json::nats::NatsTaskMessage;
use models::WebhookEndpoint;
use queries::GetWebhookSubscriptionFilterRulesQuery;

pub struct TestWebhookEndpointDeps<'a, A> {
    pub acquirer: A,
    pub clickhouse_service: &'a common::ClickHouseService,
    pub nats_client: &'a async_nats::Client,
    pub delivery_id: i64,
}

pub struct ReactivateEndpointDeps<'a, A, C: ?Sized> {
    pub acquirer: A,
    pub redis_deps: &'a C,
}

#[derive(Debug, Deserialize)]
pub struct TestWebhookEndpointCommand {
    pub endpoint_id: i64,
    pub deployment_id: i64,
    pub test_payload: Value,
}

impl TestWebhookEndpointCommand {
    pub fn new(endpoint_id: i64, deployment_id: i64, test_payload: Value) -> Self {
        Self {
            endpoint_id,
            deployment_id,
            test_payload,
        }
    }

    pub async fn execute_with_deps<'a, A>(
        self,
        deps: TestWebhookEndpointDeps<'a, A>,
    ) -> Result<TestWebhookResult, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = deps.acquirer.begin().await?;
        let endpoint = query!(
            r#"
            SELECT e.url, e.headers, e.timeout_seconds, e.app_slug, a.signing_secret
            FROM webhook_endpoints e
            JOIN webhook_apps a ON (e.deployment_id = a.deployment_id AND e.app_slug = a.app_slug)
            WHERE e.id = $1 AND e.deployment_id = $2
            "#,
            self.endpoint_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

        let app_slug = endpoint.app_slug.clone();
        let now = Utc::now();
        let payload_json = self.test_payload.to_string();
        let payload_size = payload_json.len() as i32;

        let log = WebhookLog {
            deployment_id: self.deployment_id,
            delivery_id: deps.delivery_id,
            app_slug: app_slug.clone(),
            endpoint_id: self.endpoint_id,
            event_name: "test.webhook".to_string(),
            status: "pending".to_string(),
            http_status_code: None,
            response_time_ms: None,
            attempt_number: 0,
            max_attempts: 1,
            payload: Some(payload_json.clone()),
            payload_size_bytes: payload_size,
            response_body: None,
            response_headers: None,
            request_headers: None,
            timestamp: now,
        };

        if let Err(e) = deps.clickhouse_service.insert_webhook_log(&log).await {
            tracing::warn!("Failed to log test event to Tinybird: {}", e);
        }

        let webhook_id = format!("msg_{}", deps.delivery_id);
        let webhook_timestamp = now.timestamp();
        let signature = generate_webhook_signature(
            &endpoint.signing_secret,
            &webhook_id,
            webhook_timestamp,
            &self.test_payload,
        );

        let test_filter_rules = GetWebhookSubscriptionFilterRulesQuery::new(
            self.endpoint_id,
            self.deployment_id,
            app_slug.clone(),
            "test.webhook".to_string(),
        )
        .execute_with_db(&mut *tx)
        .await?;

        query!(
            r#"
            INSERT INTO active_webhook_deliveries
            (id, endpoint_id, deployment_id, app_slug, event_name, payload, filter_rules, payload_size_bytes, webhook_id, webhook_timestamp, signature, max_attempts, attempts)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 1, 0)
            "#,
            deps.delivery_id,
            self.endpoint_id,
            self.deployment_id,
            app_slug,
            "test.webhook",
            self.test_payload.clone(),
            test_filter_rules,
            payload_size,
            webhook_id,
            webhook_timestamp,
            signature
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        let task_message = NatsTaskMessage {
            task_type: "webhook.deliver".to_string(),
            task_id: format!("webhook-test-{}", deps.delivery_id),
            payload: serde_json::json!({
                "delivery_id": deps.delivery_id,
                "deployment_id": self.deployment_id
            }),
        };

        deps.nats_client
            .publish(
                "worker.tasks.webhook.deliver",
                serde_json::to_vec(&task_message)?.into(),
            )
            .await
            .map_err(|e| AppError::Internal(format!("Failed to publish test webhook: {}", e)))?;

        Ok(TestWebhookResult {
            delivery_id: Some(deps.delivery_id),
            success: true,
            status_code: 202,
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

impl ReactivateEndpointCommand {
    pub fn new(endpoint_id: i64, deployment_id: i64) -> Self {
        Self {
            endpoint_id,
            deployment_id,
        }
    }

    pub async fn execute_with_deps<'a, A, C>(
        self,
        deps: ReactivateEndpointDeps<'a, A, C>,
    ) -> Result<WebhookEndpoint, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
        C: HasRedis + ?Sized,
    {
        let mut tx = deps.acquirer.begin().await?;
        let endpoint = query_as!(
            WebhookEndpoint,
            r#"
            UPDATE webhook_endpoints e
            SET is_active = true, updated_at = NOW()
            WHERE e.id = $1 AND e.deployment_id = $2
            RETURNING e.id as "id!",
                      e.deployment_id as "deployment_id!",
                      e.app_slug as "app_slug!",
                      e.url as "url!",
                      e.description,
                      e.headers,
                      e.is_active as "is_active!",
                      e.max_retries as "max_retries!",
                      e.timeout_seconds as "timeout_seconds!",
                      e.failure_count as "failure_count!",
                      e.last_failure_at,
                      e.auto_disabled as "auto_disabled!",
                      e.auto_disabled_at,
                      e.rate_limit_config as "rate_limit_config: serde_json::Value",
                      e.created_at as "created_at!",
                      e.updated_at as "updated_at!"
            "#,
            self.endpoint_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

        tx.commit().await?;

        ClearEndpointFailuresCommand {
            endpoint_id: self.endpoint_id,
        }
        .execute_with_deps(deps.redis_deps)
        .await?;

        Ok(endpoint)
    }
}
