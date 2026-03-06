use serde::Deserialize;
use sqlx::{query, query_as};

use common::{HasClickHouseService, HasDbRouter, error::AppError};
use dto::json::webhook_requests::{
    WebhookEndpoint, WebhookEndpointSubscription as WebhookEndpointSubscriptionDTO,
};
use models::webhook::{
    PendingDeliveryRow, WebhookApp, WebhookEndpoint as ModelWebhookEndpoint,
    WebhookEndpointSubscription,
};

#[derive(Debug, Deserialize)]
pub struct GetWebhookAppsQuery {
    deployment_id: i64,
    include_inactive: bool,
    limit: Option<i64>,
    offset: Option<i64>,
}

impl GetWebhookAppsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            include_inactive: false,
            limit: None,
            offset: None,
        }
    }

    pub fn with_inactive(mut self, include: bool) -> Self {
        self.include_inactive = include;
        self
    }

    pub fn with_pagination(mut self, limit: Option<i64>, offset: Option<i64>) -> Self {
        self.limit = limit;
        self.offset = offset;
        self
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<Vec<WebhookApp>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let limit = self.limit.unwrap_or(50);
        let offset = self.offset.unwrap_or(0);

        let apps = if self.include_inactive {
            query_as!(
                WebhookApp,
                r#"
                SELECT deployment_id as "deployment_id!",
                       app_slug as "app_slug!",
                       name as "name!",
                       description,
                       signing_secret as "signing_secret!",
                       failure_notification_emails,
                       event_catalog_slug,
                       is_active as "is_active!",
                       created_at as "created_at!",
                       updated_at as "updated_at!"
                FROM webhook_apps
                WHERE deployment_id = $1
                ORDER BY created_at DESC
                LIMIT $2 OFFSET $3
                "#,
                self.deployment_id,
                limit + 1,
                offset
            )
            .fetch_all(&mut *conn)
            .await?
        } else {
            query_as!(
                WebhookApp,
                r#"
                SELECT deployment_id as "deployment_id!",
                       app_slug as "app_slug!",
                       name as "name!",
                       description,
                       signing_secret as "signing_secret!",
                       failure_notification_emails,
                       event_catalog_slug,
                       is_active as "is_active!",
                       created_at as "created_at!",
                       updated_at as "updated_at!"
                FROM webhook_apps
                WHERE deployment_id = $1 AND is_active = true
                ORDER BY created_at DESC
                LIMIT $2 OFFSET $3
                "#,
                self.deployment_id,
                limit + 1,
                offset
            )
            .fetch_all(&mut *conn)
            .await?
        };

        Ok(apps)
    }
}

#[derive(Debug, Deserialize)]
pub struct GetWebhookEndpointsQuery {
    deployment_id: i64,
    app_slug: Option<String>,
    include_inactive: bool,
}

impl GetWebhookEndpointsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            app_slug: None,
            include_inactive: false,
        }
    }

    pub fn for_app(mut self, app_slug: String) -> Self {
        self.app_slug = Some(app_slug);
        self
    }

    pub fn with_inactive(mut self, include: bool) -> Self {
        self.include_inactive = include;
        self
    }

    pub async fn execute_with<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<Vec<ModelWebhookEndpoint>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let endpoints = match (&self.app_slug, self.include_inactive) {
            (Some(app_slug), true) => {
                query_as!(
                    ModelWebhookEndpoint,
                    r#"
                    SELECT e.id as "id!", e.deployment_id as "deployment_id!", e.app_slug as "app_slug!",
                           e.url as "url!", e.description, e.headers,
                           e.max_retries as "max_retries!", e.timeout_seconds as "timeout_seconds!", e.is_active as "is_active!",
                           e.failure_count as "failure_count!", e.last_failure_at, e.auto_disabled as "auto_disabled!", e.auto_disabled_at,
                           e.rate_limit_config,
                           e.created_at as "created_at!", e.updated_at as "updated_at!"
                    FROM webhook_endpoints e
                    WHERE e.deployment_id = $1 AND e.app_slug = $2
                    ORDER BY e.created_at DESC
                    "#,
                    self.deployment_id,
                    app_slug
                )
                .fetch_all(&mut *conn)
                .await?
            }
            (Some(app_slug), false) => {
                query_as!(
                    ModelWebhookEndpoint,
                    r#"
                    SELECT e.id as "id!", e.deployment_id as "deployment_id!", e.app_slug as "app_slug!",
                           e.url as "url!", e.description, e.headers,                            e.max_retries as "max_retries!", e.timeout_seconds as "timeout_seconds!", e.is_active as "is_active!",
                           e.failure_count as "failure_count!", e.last_failure_at, e.auto_disabled as "auto_disabled!", e.auto_disabled_at,
                           e.rate_limit_config,
                           e.created_at as "created_at!", e.updated_at as "updated_at!"
                    FROM webhook_endpoints e
                    WHERE e.deployment_id = $1 AND e.app_slug = $2 AND e.is_active = true
                    ORDER BY e.created_at DESC
                    "#,
                    self.deployment_id,
                    app_slug
                )
                .fetch_all(&mut *conn)
                .await?
            }
            (None, true) => {
                query_as!(
                    ModelWebhookEndpoint,
                    r#"
                    SELECT e.id as "id!", e.deployment_id as "deployment_id!", e.app_slug as "app_slug!",
                           e.url as "url!", e.description, e.headers,                            e.max_retries as "max_retries!", e.timeout_seconds as "timeout_seconds!", e.is_active as "is_active!",
                           e.failure_count as "failure_count!", e.last_failure_at, e.auto_disabled as "auto_disabled!", e.auto_disabled_at,
                           e.rate_limit_config,
                           e.created_at as "created_at!", e.updated_at as "updated_at!"
                    FROM webhook_endpoints e
                    WHERE e.deployment_id = $1
                    ORDER BY e.created_at DESC
                    "#,
                    self.deployment_id
                )
                .fetch_all(&mut *conn)
                .await?
            }
            (None, false) => {
                query_as!(
                    ModelWebhookEndpoint,
                    r#"
                    SELECT e.id as "id!", e.deployment_id as "deployment_id!", e.app_slug as "app_slug!",
                           e.url as "url!", e.description, e.headers,                            e.max_retries as "max_retries!", e.timeout_seconds as "timeout_seconds!", e.is_active as "is_active!",
                           e.failure_count as "failure_count!", e.last_failure_at, e.auto_disabled as "auto_disabled!", e.auto_disabled_at,
                           e.rate_limit_config,
                           e.created_at as "created_at!", e.updated_at as "updated_at!"
                    FROM webhook_endpoints e
                    WHERE e.deployment_id = $1 AND e.is_active = true
                    ORDER BY e.created_at DESC
                    "#,
                    self.deployment_id
                )
                .fetch_all(&mut *conn)
                .await?
            }
        };

        Ok(endpoints)
    }
}

#[derive(Debug, Deserialize)]
pub struct GetWebhookEndpointsWithSubscriptionsQuery {
    deployment_id: i64,
    app_slug: Option<String>,
    include_inactive: bool,
    limit: Option<i32>,
    offset: Option<i32>,
}

impl GetWebhookEndpointsWithSubscriptionsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            app_slug: None,
            include_inactive: false,
            limit: None,
            offset: None,
        }
    }

    pub fn for_app(mut self, app_slug: String) -> Self {
        self.app_slug = Some(app_slug);
        self
    }

    pub fn with_inactive(mut self, include: bool) -> Self {
        self.include_inactive = include;
        self
    }

    pub fn with_pagination(mut self, limit: Option<i32>, offset: Option<i32>) -> Self {
        self.limit = limit;
        self.offset = offset;
        self
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<Vec<WebhookEndpoint>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let limit = self.limit.unwrap_or(100) as i64;
        let offset = self.offset.unwrap_or(0) as i64;

        let endpoints = match (&self.app_slug, self.include_inactive) {
            (Some(app_slug), true) => {
                query_as!(
                    ModelWebhookEndpoint,
                    r#"
                    SELECT e.id as "id!", e.deployment_id as "deployment_id!", e.app_slug as "app_slug!",
                           e.url as "url!", e.description, e.headers,                            e.max_retries as "max_retries!", e.timeout_seconds as "timeout_seconds!", e.is_active as "is_active!",
                           e.failure_count as "failure_count!", e.last_failure_at, e.auto_disabled as "auto_disabled!", e.auto_disabled_at,
                           e.rate_limit_config,
                           e.created_at as "created_at!", e.updated_at as "updated_at!"
                    FROM webhook_endpoints e
                    WHERE e.deployment_id = $1 AND e.app_slug = $2
                    ORDER BY e.created_at DESC
                    LIMIT $3 OFFSET $4
                    "#,
                    self.deployment_id,
                    app_slug,
                    limit,
                    offset
                )
                .fetch_all(&mut *conn)
                .await?
            }
            (Some(app_slug), false) => {
                query_as!(
                    ModelWebhookEndpoint,
                    r#"
                    SELECT e.id as "id!", e.deployment_id as "deployment_id!", e.app_slug as "app_slug!",
                           e.url as "url!", e.description, e.headers,                            e.max_retries as "max_retries!", e.timeout_seconds as "timeout_seconds!", e.is_active as "is_active!",
                           e.failure_count as "failure_count!", e.last_failure_at, e.auto_disabled as "auto_disabled!", e.auto_disabled_at,
                           e.rate_limit_config,
                           e.created_at as "created_at!", e.updated_at as "updated_at!"
                    FROM webhook_endpoints e
                    WHERE e.deployment_id = $1 AND e.app_slug = $2 AND e.is_active = true
                    ORDER BY e.created_at DESC
                    "#,
                    self.deployment_id,
                    app_slug
                )
                .fetch_all(&mut *conn)
                .await?
            }
            (None, true) => {
                query_as!(
                    ModelWebhookEndpoint,
                    r#"
                    SELECT e.id as "id!", e.deployment_id as "deployment_id!", e.app_slug as "app_slug!",
                           e.url as "url!", e.description, e.headers,                            e.max_retries as "max_retries!", e.timeout_seconds as "timeout_seconds!", e.is_active as "is_active!",
                           e.failure_count as "failure_count!", e.last_failure_at, e.auto_disabled as "auto_disabled!", e.auto_disabled_at,
                           e.rate_limit_config,
                           e.created_at as "created_at!", e.updated_at as "updated_at!"
                    FROM webhook_endpoints e
                    WHERE e.deployment_id = $1
                    ORDER BY e.created_at DESC
                    "#,
                    self.deployment_id
                )
                .fetch_all(&mut *conn)
                .await?
            }
            (None, false) => {
                query_as!(
                    ModelWebhookEndpoint,
                    r#"
                    SELECT e.id as "id!", e.deployment_id as "deployment_id!", e.app_slug as "app_slug!",
                           e.url as "url!", e.description, e.headers,                            e.max_retries as "max_retries!", e.timeout_seconds as "timeout_seconds!", e.is_active as "is_active!",
                           e.failure_count as "failure_count!", e.last_failure_at, e.auto_disabled as "auto_disabled!", e.auto_disabled_at,
                           e.rate_limit_config,
                           e.created_at as "created_at!", e.updated_at as "updated_at!"
                    FROM webhook_endpoints e
                    WHERE e.deployment_id = $1 AND e.is_active = true
                    ORDER BY e.created_at DESC
                    "#,
                    self.deployment_id
                )
                .fetch_all(&mut *conn)
                .await?
            }
        };

        let mut endpoints_with_subs = Vec::new();
        for endpoint in endpoints {
            let subscriptions = query_as!(
                WebhookEndpointSubscription,
                r#"
                SELECT endpoint_id as "endpoint_id!", deployment_id as "deployment_id!", app_slug as "app_slug!",
                       event_name as "event_name!", filter_rules, created_at as "created_at!"
                FROM webhook_endpoint_subscriptions
                WHERE endpoint_id = $1 AND deployment_id = $2
                ORDER BY event_name
                "#,
                endpoint.id,
                endpoint.deployment_id
            )
            .fetch_all(&mut *conn)
            .await?;

            let subscription_dtos: Vec<WebhookEndpointSubscriptionDTO> = subscriptions
                .into_iter()
                .map(|s| WebhookEndpointSubscriptionDTO {
                    event_name: s.event_name,
                    filter_rules: s.filter_rules,
                })
                .collect();

            endpoints_with_subs.push(WebhookEndpoint {
                id: endpoint.id,
                deployment_id: endpoint.deployment_id,
                app_slug: endpoint.app_slug,
                url: endpoint.url,
                description: endpoint.description,
                headers: endpoint.headers,
                is_active: endpoint.is_active,
                max_retries: endpoint.max_retries,
                timeout_seconds: endpoint.timeout_seconds,
                failure_count: endpoint.failure_count,
                last_failure_at: endpoint.last_failure_at,
                auto_disabled: endpoint.auto_disabled,
                auto_disabled_at: endpoint.auto_disabled_at,
                rate_limit_config: endpoint.rate_limit_config,
                created_at: endpoint.created_at,
                updated_at: endpoint.updated_at,
                subscriptions: subscription_dtos,
            });
        }

        Ok(endpoints_with_subs)
    }
}

// Query for getting webhook app by name
#[derive(Debug, Deserialize)]
pub struct GetWebhookAppByNameQuery {
    deployment_id: i64,
    app_slug: String,
}

impl GetWebhookAppByNameQuery {
    pub fn new(deployment_id: i64, app_slug: String) -> Self {
        Self {
            deployment_id,
            app_slug,
        }
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<Option<WebhookApp>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let app = query_as!(
            WebhookApp,
            r#"
            SELECT deployment_id as "deployment_id!",
                   app_slug as "app_slug!",
                   name as "name!",
                   description,
                   signing_secret as "signing_secret!",
                   failure_notification_emails,
                   event_catalog_slug,
                   is_active as "is_active!",
                   created_at as "created_at!",
                   updated_at as "updated_at!"
            FROM webhook_apps
            WHERE deployment_id = $1 AND app_slug = $2
            "#,
            self.deployment_id,
            self.app_slug
        )
        .fetch_optional(&mut *conn)
        .await?;

        Ok(app)
    }
}

// Query for getting webhook events for an app
#[derive(Debug)]
pub struct GetWebhookEventsQuery {
    deployment_id: i64,
    app_slug: String,
}

impl GetWebhookEventsQuery {
    pub fn new(deployment_id: i64, app_slug: String) -> Self {
        Self {
            deployment_id,
            app_slug,
        }
    }

    pub async fn execute_with<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<Vec<models::webhook::WebhookEventDefinition>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let app = query!(
            r#"
            SELECT event_catalog_slug, created_at as "created_at!"
            FROM webhook_apps
            WHERE deployment_id = $1 AND app_slug = $2
            "#,
            self.deployment_id,
            self.app_slug
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook app not found".to_string()))?;

        if let Some(catalog_slug) = app.event_catalog_slug {
            let catalog = query!(
                r#"
                SELECT events as "events!", created_at as "created_at!"
                FROM webhook_event_catalogs
                WHERE deployment_id = $1 AND slug = $2
                "#,
                self.deployment_id,
                catalog_slug
            )
            .fetch_optional(&mut *conn)
            .await?
            .ok_or_else(|| AppError::NotFound("Event catalog not found".to_string()))?;

            let events: Vec<models::webhook::WebhookEventDefinition> =
                serde_json::from_value(catalog.events).map_err(|e| {
                    AppError::Internal(format!("Invalid catalog events format: {}", e))
                })?;

            return Ok(events);
        }

        Ok(Vec::new())
    }
}

// Query for getting webhook stats from ClickHouse
#[derive(Debug)]
pub struct GetWebhookStatsQuery {
    deployment_id: i64,
    app_slug: String,
}

impl GetWebhookStatsQuery {
    pub fn new(deployment_id: i64, app_slug: String) -> Self {
        Self {
            deployment_id,
            app_slug,
        }
    }

    pub async fn execute_with_deps<D>(&self, deps: &D) -> Result<dto::json::WebhookStats, AppError>
    where
        D: HasDbRouter + HasClickHouseService,
    {
        use dto::json::WebhookStats;

        let active_endpoints = query!(
            "SELECT COUNT(*) as count FROM webhook_endpoints WHERE deployment_id = $1 AND app_slug = $2 AND is_active = true",
            self.deployment_id,
            &self.app_slug
        )
        .fetch_one(deps.writer_pool())
        .await?
        .count
        .unwrap_or(0);

        let delivery_stats = deps
            .clickhouse_service()
            .get_webhook_delivery_stats(
                self.deployment_id,
                Some(self.app_slug.clone()),
                None,
                chrono::Utc::now() - chrono::Duration::days(30),
                chrono::Utc::now(),
            )
            .await?;

        let total = delivery_stats.total_deliveries;
        let success = delivery_stats.successful_deliveries;
        let success_rate = if total > 0 {
            (success as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        let recent_stats = deps
            .clickhouse_service()
            .get_webhook_delivery_stats(
                self.deployment_id,
                Some(self.app_slug.clone()),
                None,
                chrono::Utc::now() - chrono::Duration::hours(24),
                chrono::Utc::now(),
            )
            .await?;

        let failed_24h = recent_stats.failed_deliveries as i64;

        Ok(WebhookStats {
            total_deliveries: total,
            success_rate,
            active_endpoints,
            failed_deliveries_24h: failed_24h,
        })
    }
}

pub struct GetPendingWebhookDeliveryQuery {
    pub deployment_id: i64,
    pub delivery_id: i64,
}

impl GetPendingWebhookDeliveryQuery {
    pub fn new(deployment_id: i64, delivery_id: i64) -> Self {
        Self {
            deployment_id,
            delivery_id,
        }
    }

    pub async fn execute_with<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<dto::clickhouse::webhook::WebhookLog, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let row = sqlx::query_as::<_, PendingDeliveryRow>(
            r#"
            SELECT
                d.id as delivery_id,
                d.deployment_id,
                d.app_slug,
                d.endpoint_id,
                e.url as endpoint_url,
                d.event_name,
                d.payload,
                d.attempts as attempt_number,
                d.max_attempts,
                d.created_at as timestamp
            FROM active_webhook_deliveries d
            JOIN webhook_endpoints e ON e.id = d.endpoint_id
            WHERE d.id = $1 AND d.deployment_id = $2
            "#,
        )
        .bind(self.delivery_id)
        .bind(self.deployment_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound("Pending delivery not found".to_string()))?;

        let payload_json = row
            .payload
            .map(|p| serde_json::to_string(&p).unwrap_or_default());
        Ok(dto::clickhouse::webhook::WebhookLog {
            deployment_id: row.deployment_id,
            delivery_id: row.delivery_id,
            app_slug: row.app_slug,
            endpoint_id: row.endpoint_id,
            event_name: row.event_name,
            status: "pending".to_string(),
            http_status_code: None,
            response_time_ms: None,
            attempt_number: row.attempt_number,
            max_attempts: row.max_attempts,
            payload: payload_json.clone(),
            payload_size_bytes: payload_json.as_ref().map(|p| p.len() as i32).unwrap_or(0),
            response_body: None,
            response_headers: None,
            timestamp: row.timestamp,
            request_headers: None,
        })
    }
}
