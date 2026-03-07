use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::query;

use common::error::AppError;
use common::utils::webhook::generate_webhook_signature;
use dto::clickhouse::webhook::WebhookLog;
use dto::json::nats::NatsTaskMessage;
use queries::GetWebhookSubscriptionFilterRulesQuery;

use super::{
    GetSubscribedEndpointsCommand,
    webhook_subscription::{GetSubscribedEndpointsDeps, evaluate_filter},
};

pub struct TriggerWebhookEventDeps<'a, IdFn> {
    pub db_router: &'a common::DbRouter,
    pub redis_client: &'a redis::Client,
    pub clickhouse_service: &'a common::ClickHouseService,
    pub nats_client: &'a async_nats::Client,
    pub id_gen: IdFn,
}

pub struct ReplayWebhookDeliveryDeps<'a, IdFn> {
    pub db_router: &'a common::DbRouter,
    pub clickhouse_service: &'a common::ClickHouseService,
    pub nats_client: &'a async_nats::Client,
    pub id_gen: IdFn,
}

#[derive(Debug, Deserialize)]
pub struct TriggerWebhookEventCommand {
    pub deployment_id: i64,
    pub app_slug: String,
    pub event_name: String,
    pub payload: Value,
    pub filter_context: Option<Value>,
}

impl TriggerWebhookEventCommand {
    pub fn new(deployment_id: i64, app_slug: String, event_name: String, payload: Value) -> Self {
        Self {
            deployment_id,
            app_slug,
            event_name,
            payload,
            filter_context: None,
        }
    }

    pub fn with_filter_context(mut self, context: Value) -> Self {
        self.filter_context = Some(context);
        self
    }

    pub async fn execute_with_deps<IdFn>(
        self,
        deps: TriggerWebhookEventDeps<'_, IdFn>,
    ) -> Result<TriggerWebhookEventResult, AppError>
    where
        IdFn: Fn() -> Result<i64, AppError> + Copy,
    {
        let pool = deps.db_router.writer();
        let app_info = query!(
            r#"
            SELECT app_slug, name
            FROM webhook_apps
            WHERE deployment_id = $1 AND app_slug = $2 AND is_active = true
            "#,
            self.deployment_id,
            self.app_slug
        )
        .fetch_optional(pool)
        .await?;

        let app_info = match app_info {
            Some(app) => app,
            None => return Err(AppError::NotFound("Webhook app not found".to_string())),
        };

        let payload_size = self.payload.to_string().len() as i32;
        let app_slug = app_info.app_slug.clone();

        let endpoints = GetSubscribedEndpointsCommand::new(
            self.deployment_id,
            app_slug.clone(),
            self.event_name.clone(),
        )
        .execute_with_deps(GetSubscribedEndpointsDeps {
            executor: pool,
            redis_client: deps.redis_client,
        })
        .await?;

        let mut delivery_ids = Vec::new();
        let mut filtered_count = 0usize;

        for endpoint in endpoints {
            if let Some(filter_rules) = &endpoint.filter_rules {
                if !evaluate_filter(filter_rules, &self.payload) {
                    filtered_count += 1;
                    continue;
                }
            }

            let delivery_id = (deps.id_gen)()?;
            let webhook_id = format!("msg_{}", delivery_id);
            let webhook_timestamp = Utc::now().timestamp();
            let signature = generate_webhook_signature(
                &endpoint.signing_secret,
                &webhook_id,
                webhook_timestamp,
                &self.payload,
            );

            let delivery = query!(
                r#"
                INSERT INTO active_webhook_deliveries
                (id, endpoint_id, deployment_id, app_slug, event_name, payload, filter_rules, payload_size_bytes, webhook_id, webhook_timestamp, signature, max_attempts)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
                RETURNING id
                "#,
                delivery_id,
                endpoint.id,
                self.deployment_id,
                app_slug.clone(),
                self.event_name,
                self.payload.clone(),
                endpoint.filter_rules.clone(),
                self.payload.to_string().len() as i32,
                webhook_id,
                webhook_timestamp,
                signature,
                endpoint.max_retries
            )
            .fetch_one(pool)
            .await?;

            delivery_ids.push(delivery.id);

            let payload_json = serde_json::to_string(&self.payload).unwrap_or_default();
            let ch_log = WebhookLog {
                deployment_id: self.deployment_id,
                delivery_id: delivery.id,
                app_slug: app_slug.clone(),
                endpoint_id: endpoint.id,
                event_name: self.event_name.clone(),
                status: "pending".to_string(),
                http_status_code: None,
                response_time_ms: None,
                attempt_number: 0,
                max_attempts: endpoint.max_retries,
                payload: Some(payload_json),
                payload_size_bytes: payload_size,
                response_body: None,
                response_headers: None,
                request_headers: None,
                timestamp: Utc::now(),
            };

            if let Err(e) = deps.clickhouse_service.insert_webhook_log(&ch_log).await {
                tracing::warn!("Failed to log pending delivery to Tinybird: {}", e);
            }

            let task_message = NatsTaskMessage {
                task_type: "webhook.deliver".to_string(),
                task_id: format!("webhook-{}-{}", delivery.id, self.deployment_id),
                payload: serde_json::json!({
                    "delivery_id": delivery.id,
                    "deployment_id": self.deployment_id
                }),
            };

            deps.nats_client
                .publish(
                    "worker.tasks.webhook.deliver",
                    serde_json::to_vec(&task_message)
                        .map_err(|e| {
                            AppError::Internal(format!("Failed to serialize task: {}", e))
                        })?
                        .into(),
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

#[derive(Debug, Serialize)]
pub struct TriggerWebhookEventResult {
    pub delivery_ids: Vec<i64>,
    pub filtered_count: usize,
    pub delivered_count: usize,
}

#[derive(Debug, Deserialize)]
pub struct ReplayWebhookDeliveryCommand {
    pub delivery_id: i64,
    pub deployment_id: i64,
}

impl ReplayWebhookDeliveryCommand {
    pub async fn execute_with_deps<IdFn>(
        self,
        deps: ReplayWebhookDeliveryDeps<'_, IdFn>,
    ) -> Result<i64, AppError>
    where
        IdFn: Fn() -> Result<i64, AppError> + Copy,
    {
        let pool = deps.db_router.writer();
        tracing::info!(
            "Starting replay for delivery_id: {}, deployment_id: {}",
            self.delivery_id,
            self.deployment_id
        );

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
        .fetch_optional(pool)
        .await?;

        if is_active.is_some() {
            return Err(AppError::BadRequest(
                "Cannot replay an active delivery. Please wait for it to complete or cancel it first.".to_string()
            ));
        }

        let replay_source = deps
            .clickhouse_service
            .get_webhook_replay_source(self.deployment_id, self.delivery_id)
            .await?;

        let payload_raw = replay_source.payload.ok_or_else(|| {
            AppError::BadRequest("Cannot replay delivery without payload".to_string())
        })?;
        let payload: Value = serde_json::from_str(&payload_raw)
            .map_err(|e| AppError::BadRequest(format!("Invalid replay payload JSON: {}", e)))?;

        let endpoint_id = replay_source.endpoint_id;
        let event_name = replay_source.event_name;
        let app_slug = replay_source.app_slug;

        tracing::info!("Looking up endpoint with id: {} for replay", endpoint_id);
        let endpoint = query!(
            r#"
            SELECT e.id, e.url, e.max_retries, a.signing_secret, e.deployment_id
            FROM webhook_endpoints e
            JOIN webhook_apps a ON (e.deployment_id = a.deployment_id AND e.app_slug = a.app_slug)
            WHERE e.id = $1
            "#,
            endpoint_id
        )
        .fetch_optional(pool)
        .await?;

        let endpoint = endpoint.ok_or_else(|| {
            tracing::warn!(
                "Endpoint {} no longer exists - it may have been deleted",
                endpoint_id
            );
            AppError::NotFound(
                "Cannot replay delivery: The webhook endpoint has been deleted. Please create a new endpoint and trigger the event again.".to_string(),
            )
        })?;

        if endpoint.deployment_id != self.deployment_id {
            tracing::error!(
                "Deployment mismatch: endpoint belongs to deployment {}, but replay requested for deployment {}",
                endpoint.deployment_id,
                self.deployment_id
            );
            return Err(AppError::BadRequest(
                "Endpoint belongs to different deployment".to_string(),
            ));
        }

        let endpoint_active = query!(
            r#"
            SELECT is_active
            FROM webhook_endpoints
            WHERE id = $1
            "#,
            endpoint_id
        )
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

        if !endpoint_active.is_active.unwrap_or(false) {
            return Err(AppError::BadRequest(
                "Cannot replay delivery to inactive endpoint. Reactivate the endpoint first."
                    .to_string(),
            ));
        }

        let current_filter_rules = GetWebhookSubscriptionFilterRulesQuery::new(
            endpoint_id,
            self.deployment_id,
            app_slug.clone(),
            event_name.clone(),
        )
        .execute_with_db(pool)
        .await?
        .ok_or_else(|| {
            AppError::BadRequest(
                "Cannot replay delivery: endpoint is no longer subscribed to this event."
                    .to_string(),
            )
        })?;

        let new_delivery_id = (deps.id_gen)()?;
        let webhook_id = format!("msg_{}", new_delivery_id);
        let webhook_timestamp = Utc::now().timestamp();
        let signature = Some(generate_webhook_signature(
            &endpoint.signing_secret,
            &webhook_id,
            webhook_timestamp,
            &payload,
        ));
        let payload_size_bytes = serde_json::to_string(&payload).unwrap_or_default().len() as i32;
        let max_attempts = replay_source.max_attempts.max(1);

        tracing::info!(
            original_delivery_id = self.delivery_id,
            new_delivery_id,
            deployment_id = self.deployment_id,
            endpoint_id,
            event_name = %event_name,
            "Replay queued with snapshot of current subscription filter rules",
        );

        let new_delivery = query!(
            r#"
            INSERT INTO active_webhook_deliveries
            (id, endpoint_id, deployment_id, app_slug, event_name, payload, filter_rules, payload_size_bytes, webhook_id, webhook_timestamp, signature, max_attempts, attempts)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 0)
            RETURNING id
            "#,
            new_delivery_id,
            endpoint_id,
            self.deployment_id,
            app_slug.clone(),
            event_name.clone(),
            payload,
            current_filter_rules,
            payload_size_bytes,
            webhook_id,
            webhook_timestamp,
            signature,
            max_attempts
        )
        .fetch_one(pool)
        .await?;

        let task_message = NatsTaskMessage {
            task_type: "webhook.deliver".to_string(),
            task_id: format!("webhook-replay-{}", new_delivery.id),
            payload: serde_json::json!({
                "delivery_id": new_delivery.id,
                "deployment_id": self.deployment_id
            }),
        };

        deps.nats_client
            .publish(
                "worker.tasks.webhook.deliver",
                serde_json::to_vec(&task_message)?.into(),
            )
            .await
            .map_err(|e| AppError::Internal(format!("Failed to publish replay to NATS: {}", e)))?;

        Ok(new_delivery.id)
    }
}
