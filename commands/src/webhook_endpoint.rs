use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{query, query_as};
use std::collections::{HashMap, HashSet};

use crate::Command;
use crate::webhook_delivery::ClearEndpointFailuresCommand;
use common::error::AppError;
use common::state::AppState;
use common::utils::ssrf::validate_webhook_url;
use common::utils::webhook::generate_webhook_signature;
use dto::clickhouse::webhook::WebhookLog;
use dto::json::nats::NatsTaskMessage;
use models::WebhookEndpoint;
use queries::{GetWebhookEventsQuery, Query};

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

const FILTER_LOGICAL_OPERATORS: [&str; 2] = ["$and", "$or"];
const FILTER_FIELD_OPERATORS: [&str; 10] = [
    "$eq",
    "$ne",
    "$gt",
    "$gte",
    "$lt",
    "$lte",
    "$in",
    "$nin",
    "$contains",
    "$exists",
];
const MAX_ENDPOINT_RETRY_WINDOW_SECONDS: i64 = 7 * 24 * 60 * 60;

fn retry_delay_seconds(attempts: i32) -> i64 {
    match attempts {
        1 => 30,
        2 => 60,
        3 => 5 * 60,
        4 => 15 * 60,
        _ => 6 * 60 * 60,
    }
}

fn max_attempts_for_retry_window(max_window_seconds: i64) -> i32 {
    let mut attempts: i32 = 1;
    let mut total_seconds: i64 = 0;

    loop {
        let delay = retry_delay_seconds(attempts);
        if total_seconds + delay > max_window_seconds {
            break;
        }
        total_seconds += delay;
        attempts += 1;
    }

    attempts
}

fn validate_endpoint_max_retries(max_retries: i32, max_allowed: i32) -> Result<(), AppError> {
    if max_retries < 1 {
        return Err(AppError::BadRequest(
            "max_retries must be at least 1".to_string(),
        ));
    }
    if max_retries > max_allowed {
        return Err(AppError::BadRequest(format!(
            "max_retries cannot exceed {} (7-day retry window limit)",
            max_allowed
        )));
    }
    Ok(())
}

fn collect_schema_paths(schema: &Value, prefix: Option<&str>, paths: &mut HashSet<String>) {
    let Some(schema_obj) = schema.as_object() else {
        return;
    };

    let Some(properties) = schema_obj.get("properties").and_then(Value::as_object) else {
        return;
    };

    for (field_name, field_schema) in properties {
        let current_path = match prefix {
            Some(parent) if !parent.is_empty() => format!("{}.{}", parent, field_name),
            _ => field_name.clone(),
        };

        paths.insert(current_path.clone());
        collect_schema_paths(field_schema, Some(&current_path), paths);
    }
}

fn validate_filter_condition(condition: &Value, path_ctx: &str) -> Result<(), AppError> {
    let Some(operators) = condition.as_object() else {
        return Ok(());
    };

    for (op, expected) in operators {
        if !FILTER_FIELD_OPERATORS.contains(&op.as_str()) {
            return Err(AppError::BadRequest(format!(
                "Unsupported filter operator '{}' at {}",
                op, path_ctx
            )));
        }

        match op.as_str() {
            "$in" | "$nin" => {
                if !expected.is_array() {
                    return Err(AppError::BadRequest(format!(
                        "Operator '{}' expects an array at {}",
                        op, path_ctx
                    )));
                }
            }
            "$exists" => {
                if !expected.is_boolean() {
                    return Err(AppError::BadRequest(format!(
                        "Operator '$exists' expects a boolean at {}",
                        path_ctx
                    )));
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn validate_filter_rules(
    filter_rules: &Value,
    allowed_paths: Option<&HashSet<String>>,
    path_ctx: &str,
) -> Result<(), AppError> {
    let rules = filter_rules.as_object().ok_or_else(|| {
        AppError::BadRequest(format!(
            "Filter rules must be a JSON object for {}",
            path_ctx
        ))
    })?;

    for (key, value) in rules {
        if FILTER_LOGICAL_OPERATORS.contains(&key.as_str()) {
            let conditions = value.as_array().ok_or_else(|| {
                AppError::BadRequest(format!(
                    "Logical operator '{}' expects an array at {}",
                    key, path_ctx
                ))
            })?;

            if conditions.is_empty() {
                return Err(AppError::BadRequest(format!(
                    "Logical operator '{}' cannot be empty at {}",
                    key, path_ctx
                )));
            }

            for (idx, nested) in conditions.iter().enumerate() {
                validate_filter_rules(
                    nested,
                    allowed_paths,
                    &format!("{}.{}[{}]", path_ctx, key, idx),
                )?;
            }
            continue;
        }

        if let Some(paths) = allowed_paths {
            if !paths.contains(key) {
                return Err(AppError::BadRequest(format!(
                    "Unknown filter field '{}' at {}",
                    key, path_ctx
                )));
            }
        }

        validate_filter_condition(value, &format!("{}.{}", path_ctx, key))?;
    }

    Ok(())
}

async fn load_event_schema_map(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: &str,
) -> Result<HashMap<String, (bool, Option<HashSet<String>>)>, AppError> {
    let events = GetWebhookEventsQuery::new(deployment_id, app_slug.to_string())
        .execute(app_state)
        .await?;

    let mut event_map: HashMap<String, (bool, Option<HashSet<String>>)> =
        HashMap::with_capacity(events.len());

    for event in events {
        let allowed_paths = event.schema.as_ref().map(|schema| {
            let mut paths = HashSet::new();
            collect_schema_paths(schema, None, &mut paths);
            paths
        });
        event_map.insert(event.name, (event.is_archived, allowed_paths));
    }

    Ok(event_map)
}

async fn validate_event_subscriptions(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: &str,
    subscriptions: &[EventSubscriptionData],
) -> Result<(), AppError> {
    if subscriptions.is_empty() {
        return Err(AppError::BadRequest(
            "At least one subscription is required".to_string(),
        ));
    }

    let event_map = load_event_schema_map(app_state, deployment_id, app_slug).await?;

    for sub in subscriptions {
        let event_name = sub.event_name.trim();
        if event_name.is_empty() {
            return Err(AppError::BadRequest(
                "Subscription event_name is required".to_string(),
            ));
        }

        let Some((is_archived, allowed_paths)) = event_map.get(event_name) else {
            return Err(AppError::BadRequest(format!(
                "Unknown event '{}' for app '{}'",
                event_name, app_slug
            )));
        };

        if *is_archived {
            return Err(AppError::BadRequest(format!(
                "Event '{}' is archived and cannot be subscribed",
                event_name
            )));
        }

        if let Some(filter_rules) = &sub.filter_rules {
            validate_filter_rules(
                filter_rules,
                allowed_paths.as_ref(),
                &format!("subscriptions.{}", event_name),
            )?;
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct CreateWebhookEndpointCommand {
    pub deployment_id: i64,
    pub app_slug: String,
    pub url: String,
    pub description: Option<String>,
    pub headers: Option<Value>,
    pub subscriptions: Vec<EventSubscriptionData>,
    pub max_retries: Option<i32>,
    pub timeout_seconds: Option<i32>,
    pub rate_limit_config: Option<Value>,
}

impl CreateWebhookEndpointCommand {
    pub fn new(deployment_id: i64, app_slug: String, url: String) -> Self {
        Self {
            deployment_id,
            app_slug,
            url,
            description: None,
            headers: None,
            subscriptions: Vec::new(),
            max_retries: None,
            timeout_seconds: None,
            rate_limit_config: None,
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
        url::Url::parse(&self.url)
            .map_err(|_| AppError::BadRequest("Invalid webhook URL".to_string()))?;

        validate_webhook_url(&self.url)
            .map_err(|e| AppError::BadRequest(format!("Invalid webhook URL: {}", e)))?;

        validate_event_subscriptions(
            app_state,
            self.deployment_id,
            &self.app_slug,
            &self.subscriptions,
        )
        .await?;

        let max_allowed_retries = max_attempts_for_retry_window(MAX_ENDPOINT_RETRY_WINDOW_SECONDS);
        let endpoint_max_retries = self.max_retries.unwrap_or(max_allowed_retries);
        validate_endpoint_max_retries(endpoint_max_retries, max_allowed_retries)?;

        let mut tx = app_state.db_pool.begin().await?;

        let endpoint_id = app_state.sf.next_id()? as i64;
        let headers_json = self.headers.unwrap_or_else(|| serde_json::json!({}));

        let endpoint = query_as!(
            WebhookEndpoint,
            r#"
            INSERT INTO webhook_endpoints (id, deployment_id, app_slug, url, description, headers, max_retries, timeout_seconds, is_active, rate_limit_config)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, true, $9)
            RETURNING id as "id!",
                      deployment_id as "deployment_id!",
                      app_slug as "app_slug!",
                      url as "url!",
                      description,
                      headers,
                      is_active as "is_active!",
                      max_retries as "max_retries!",
                      timeout_seconds as "timeout_seconds!",
                      failure_count as "failure_count!",
                      last_failure_at,
                      auto_disabled as "auto_disabled!",
                      auto_disabled_at,
                      rate_limit_config,
                      created_at as "created_at!",
                      updated_at as "updated_at!"
            "#,
            endpoint_id,
            self.deployment_id,
            self.app_slug,
            self.url,
            self.description,
            headers_json,
            endpoint_max_retries,
            self.timeout_seconds.unwrap_or(30),
            self.rate_limit_config
        )
        .fetch_one(&mut *tx)
        .await?;

        // Subscribe to events with individual filter rules
        for subscription in self.subscriptions {
            query!(
                r#"
                INSERT INTO webhook_endpoint_subscriptions (endpoint_id, deployment_id, app_slug, event_name, filter_rules)
                VALUES ($1, $2, $3, $4, $5)
                "#,
                endpoint.id,
                self.deployment_id,
                self.app_slug,
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
    pub rate_limit_config: Option<Value>,
}

impl Command for UpdateWebhookEndpointCommand {
    type Output = WebhookEndpoint;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        if let Some(ref url) = self.url {
            url::Url::parse(url)
                .map_err(|_| AppError::BadRequest("Invalid webhook URL".to_string()))?;

            validate_webhook_url(url)
                .map_err(|e| AppError::BadRequest(format!("Invalid webhook URL: {}", e)))?;
        }

        let max_allowed_retries = max_attempts_for_retry_window(MAX_ENDPOINT_RETRY_WINDOW_SECONDS);
        if let Some(max_retries) = self.max_retries {
            validate_endpoint_max_retries(max_retries, max_allowed_retries)?;
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
                timeout_seconds = COALESCE($8, e.timeout_seconds),
                rate_limit_config = COALESCE($9, e.rate_limit_config)
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
                      e.rate_limit_config,
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
            self.timeout_seconds,
            self.rate_limit_config
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

        if let Some(subscriptions) = self.subscriptions {
            validate_event_subscriptions(
                app_state,
                self.deployment_id,
                &endpoint.app_slug,
                &subscriptions,
            )
            .await?;

            query!(
                r#"
                DELETE FROM webhook_endpoint_subscriptions
                WHERE endpoint_id = $1
                "#,
                self.endpoint_id
            )
            .execute(&mut *tx)
            .await?;

            for subscription in subscriptions {
                query!(
                    r#"
                    INSERT INTO webhook_endpoint_subscriptions (endpoint_id, deployment_id, app_slug, event_name, filter_rules)
                    VALUES ($1, $2, $3, $4, $5)
                    "#,
                    self.endpoint_id,
                    self.deployment_id,
                    endpoint.app_slug.clone(),
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

        let app_slug = query!(
            r#"
            SELECT e.app_slug
            FROM webhook_endpoints e
            WHERE e.id = $1 AND e.deployment_id = $2
            "#,
            self.endpoint_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?
        .app_slug;

        let subscriptions_for_validation = self
            .subscribe_to_events
            .iter()
            .map(|event_name| EventSubscriptionData {
                event_name: event_name.clone(),
                filter_rules: self.filter_rules.clone(),
            })
            .collect::<Vec<_>>();

        validate_event_subscriptions(
            app_state,
            self.deployment_id,
            &app_slug,
            &subscriptions_for_validation,
        )
        .await?;

        query!(
            r#"
            DELETE FROM webhook_endpoint_subscriptions
            WHERE endpoint_id = $1
            "#,
            self.endpoint_id
        )
        .execute(&mut *tx)
        .await?;

        for event_name in &self.subscribe_to_events {
            query!(
                r#"
                INSERT INTO webhook_endpoint_subscriptions (endpoint_id, deployment_id, app_slug, event_name, filter_rules)
                VALUES ($1, $2, $3, $4, $5)
                "#,
                self.endpoint_id,
                self.deployment_id,
                app_slug.clone(),
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
        .fetch_optional(&app_state.db_pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

        let app_slug = endpoint.app_slug.clone();

        let delivery_id = app_state.sf.next_id()? as i64;
        let now = Utc::now();

        let payload_json = serde_json::to_string(&self.test_payload).unwrap_or_default();
        let payload_size = payload_json.len() as i32;

        let log = WebhookLog {
            deployment_id: self.deployment_id,
            delivery_id,
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

        if let Err(e) = app_state.clickhouse_service.insert_webhook_log(&log).await {
            tracing::warn!("Failed to log test event to Tinybird: {}", e);
        }

        let webhook_id = format!("msg_{}", delivery_id);
        let webhook_timestamp = now.timestamp();
        let signature = generate_webhook_signature(
            &endpoint.signing_secret,
            &webhook_id,
            webhook_timestamp,
            &self.test_payload,
        );

        let test_subscription = query!(
            r#"
            SELECT filter_rules
            FROM webhook_endpoint_subscriptions
            WHERE endpoint_id = $1 AND deployment_id = $2 AND app_slug = $3 AND event_name = $4
            "#,
            self.endpoint_id,
            self.deployment_id,
            app_slug,
            "test.webhook"
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        query!(
            r#"
            INSERT INTO active_webhook_deliveries
            (id, endpoint_id, deployment_id, app_slug, event_name, payload, filter_rules, payload_size_bytes, webhook_id, webhook_timestamp, signature, max_attempts, attempts)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 1, 0)
            "#,
            delivery_id,
            self.endpoint_id,
            self.deployment_id,
            app_slug,
            "test.webhook",
            self.test_payload.clone(),
            test_subscription.and_then(|s| s.filter_rules),
            payload_size,
            webhook_id,
            webhook_timestamp,
            signature
        )
        .execute(&app_state.db_pool)
        .await?;

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

        Ok(TestWebhookResult {
            delivery_id: Some(delivery_id),
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
        .fetch_optional(&app_state.db_pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

        ClearEndpointFailuresCommand {
            endpoint_id: self.endpoint_id,
        }
        .execute(app_state)
        .await?;

        Ok(endpoint)
    }
}
