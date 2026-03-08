use serde::Deserialize;
use serde_json::Value;
use sqlx::{query, query_as};

use common::{HasDbRouter, HasIdGenerator, error::AppError};
use common::utils::ssrf::validate_webhook_url;
use models::WebhookEndpoint;

use super::validation::{
    EventSubscriptionData, MAX_ENDPOINT_RETRY_WINDOW_SECONDS, max_attempts_for_retry_window,
    validate_endpoint_max_retries, validate_event_subscriptions,
};

async fn replace_endpoint_subscriptions(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    endpoint_id: i64,
    deployment_id: i64,
    app_slug: &str,
    subscriptions: &[EventSubscriptionData],
) -> Result<(), AppError> {
    query(
        r#"
        DELETE FROM webhook_endpoint_subscriptions
        WHERE endpoint_id = $1
        "#,
    )
    .bind(endpoint_id)
    .execute(&mut **tx)
    .await?;

    for subscription in subscriptions {
        query(
            r#"
            INSERT INTO webhook_endpoint_subscriptions (endpoint_id, deployment_id, app_slug, event_name, filter_rules)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(endpoint_id)
        .bind(deployment_id)
        .bind(app_slug)
        .bind(&subscription.event_name)
        .bind(&subscription.filter_rules)
        .execute(&mut **tx)
        .await?;
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

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }

    pub fn with_subscriptions(mut self, subscriptions: Vec<EventSubscriptionData>) -> Self {
        self.subscriptions = subscriptions;
        self
    }

    pub fn with_headers(mut self, headers: Option<Value>) -> Self {
        self.headers = headers;
        self
    }

    pub fn with_max_retries(mut self, max_retries: Option<i32>) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn with_timeout_seconds(mut self, timeout_seconds: Option<i32>) -> Self {
        self.timeout_seconds = timeout_seconds;
        self
    }

    pub fn with_rate_limit_config(mut self, rate_limit_config: Option<Value>) -> Self {
        self.rate_limit_config = rate_limit_config;
        self
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<WebhookEndpoint, AppError>
    where
        D: HasDbRouter + HasIdGenerator + ?Sized,
    {
        url::Url::parse(&self.url)
            .map_err(|_| AppError::BadRequest("Invalid webhook URL".to_string()))?;

        validate_webhook_url(&self.url)
            .map_err(|e| AppError::BadRequest(format!("Invalid webhook URL: {}", e)))?;

        validate_event_subscriptions(
            deps.db_router(),
            self.deployment_id,
            &self.app_slug,
            &self.subscriptions,
        )
        .await?;

        let max_allowed_retries = max_attempts_for_retry_window(MAX_ENDPOINT_RETRY_WINDOW_SECONDS);
        let endpoint_max_retries = self.max_retries.unwrap_or(max_allowed_retries);
        validate_endpoint_max_retries(endpoint_max_retries, max_allowed_retries)?;

        let mut tx = deps.writer_pool().begin().await?;
        let endpoint_id = deps.id_generator().next_id()? as i64;

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

impl UpdateWebhookEndpointCommand {
    pub fn new(endpoint_id: i64, deployment_id: i64) -> Self {
        Self {
            endpoint_id,
            deployment_id,
            url: None,
            description: None,
            headers: None,
            is_active: None,
            max_retries: None,
            timeout_seconds: None,
            subscriptions: None,
            rate_limit_config: None,
        }
    }

    pub fn with_url(mut self, url: Option<String>) -> Self {
        self.url = url;
        self
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }

    pub fn with_headers(mut self, headers: Option<Value>) -> Self {
        self.headers = headers;
        self
    }

    pub fn with_is_active(mut self, is_active: Option<bool>) -> Self {
        self.is_active = is_active;
        self
    }

    pub fn with_max_retries(mut self, max_retries: Option<i32>) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn with_timeout_seconds(mut self, timeout_seconds: Option<i32>) -> Self {
        self.timeout_seconds = timeout_seconds;
        self
    }

    pub fn with_subscriptions(mut self, subscriptions: Option<Vec<EventSubscriptionData>>) -> Self {
        self.subscriptions = subscriptions;
        self
    }

    pub fn with_rate_limit_config(mut self, rate_limit_config: Option<Value>) -> Self {
        self.rate_limit_config = rate_limit_config;
        self
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<WebhookEndpoint, AppError>
    where
        D: HasDbRouter + ?Sized,
    {
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

        let mut tx = deps.writer_pool().begin().await?;

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
                deps.db_router(),
                self.deployment_id,
                &endpoint.app_slug,
                &subscriptions,
            )
            .await?;

            replace_endpoint_subscriptions(
                &mut tx,
                self.endpoint_id,
                self.deployment_id,
                &endpoint.app_slug,
                &subscriptions,
            )
            .await?;
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

impl UpdateEndpointSubscriptionsCommand {
    pub fn new(endpoint_id: i64, deployment_id: i64, subscribe_to_events: Vec<String>) -> Self {
        Self {
            endpoint_id,
            deployment_id,
            subscribe_to_events,
            filter_rules: None,
        }
    }

    pub fn with_filter_rules(mut self, filter_rules: Option<Value>) -> Self {
        self.filter_rules = filter_rules;
        self
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Vec<String>, AppError>
    where
        D: HasDbRouter + ?Sized,
    {
        let mut tx = deps.writer_pool().begin().await?;

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
            deps.db_router(),
            self.deployment_id,
            &app_slug,
            &subscriptions_for_validation,
        )
        .await?;

        let subscriptions = self
            .subscribe_to_events
            .iter()
            .map(|event_name| EventSubscriptionData {
                event_name: event_name.clone(),
                filter_rules: self.filter_rules.clone(),
            })
            .collect::<Vec<_>>();

        replace_endpoint_subscriptions(
            &mut tx,
            self.endpoint_id,
            self.deployment_id,
            &app_slug,
            &subscriptions,
        )
        .await?;

        tx.commit().await?;
        Ok(self.subscribe_to_events)
    }
}

#[derive(Debug, Deserialize)]
pub struct DeleteWebhookEndpointCommand {
    pub endpoint_id: i64,
    pub deployment_id: i64,
}

impl DeleteWebhookEndpointCommand {
    pub fn new(endpoint_id: i64, deployment_id: i64) -> Self {
        Self {
            endpoint_id,
            deployment_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = query!(
            r#"
            DELETE FROM webhook_endpoints e
            WHERE e.id = $1 AND e.deployment_id = $2
            "#,
            self.endpoint_id,
            self.deployment_id
        )
        .execute(executor)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Webhook endpoint not found".to_string()));
        }

        Ok(())
    }
}
