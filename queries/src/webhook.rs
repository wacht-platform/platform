use serde::Deserialize;
use sqlx::{Row, query, query_as};

use common::{HasClickHouseService, HasDbRouter, error::AppError};
use dto::json::webhook_requests::{
    WebhookEndpoint, WebhookEndpointSubscription as WebhookEndpointSubscriptionDTO,
};
use models::webhook::{
    PendingDeliveryRow, WebhookApp, WebhookEndpoint as ModelWebhookEndpoint, WebhookEventCatalog,
};

fn parse_endpoint_subscriptions(
    value: serde_json::Value,
) -> Result<Vec<WebhookEndpointSubscriptionDTO>, AppError> {
    let array = value.as_array().ok_or_else(|| {
        AppError::Internal("Invalid endpoint subscriptions format: expected array".to_string())
    })?;

    let mut subscriptions = Vec::with_capacity(array.len());
    for item in array {
        let event_name = item
            .get("event_name")
            .and_then(|x| x.as_str())
            .ok_or_else(|| {
                AppError::Internal(
                    "Invalid endpoint subscriptions format: missing event_name".to_string(),
                )
            })?
            .to_string();

        subscriptions.push(WebhookEndpointSubscriptionDTO {
            event_name,
            filter_rules: item.get("filter_rules").cloned(),
        });
    }

    Ok(subscriptions)
}

fn map_webhook_endpoint_row(
    row: sqlx::postgres::PgRow,
    subscriptions: Vec<WebhookEndpointSubscriptionDTO>,
) -> WebhookEndpoint {
    WebhookEndpoint {
        id: row.get("id"),
        deployment_id: row.get("deployment_id"),
        app_slug: row.get("app_slug"),
        url: row.get("url"),
        description: row.get("description"),
        headers: row.get("headers"),
        is_active: row.get("is_active"),
        max_retries: row.get("max_retries"),
        timeout_seconds: row.get("timeout_seconds"),
        failure_count: row.get("failure_count"),
        last_failure_at: row.get("last_failure_at"),
        auto_disabled: row.get("auto_disabled"),
        auto_disabled_at: row.get("auto_disabled_at"),
        rate_limit_config: row.get("rate_limit_config"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        subscriptions,
    }
}

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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<WebhookApp>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
            .fetch_all(executor)
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
            .fetch_all(executor)
            .await?
        };

        Ok(apps)
    }
}

#[derive(Debug, Deserialize)]
pub struct GetEventCatalogQuery {
    pub deployment_id: i64,
    pub slug: String,
}

impl GetEventCatalogQuery {
    pub fn new(deployment_id: i64, slug: String) -> Self {
        Self {
            deployment_id,
            slug,
        }
    }

    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Option<WebhookEventCatalog>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let catalog = query_as!(
            WebhookEventCatalog,
            r#"
            SELECT deployment_id as "deployment_id!",
                   slug as "slug!",
                   name as "name!",
                   description,
                   events as "events!",
                   created_at as "created_at!",
                   updated_at as "updated_at!"
            FROM webhook_event_catalogs
            WHERE deployment_id = $1 AND slug = $2
            "#,
            self.deployment_id,
            self.slug
        )
        .fetch_optional(executor)
        .await?;

        Ok(catalog)
    }
}

#[derive(Debug, Deserialize)]
pub struct ListEventCatalogsQuery {
    pub deployment_id: i64,
}

impl ListEventCatalogsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }

    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Vec<WebhookEventCatalog>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let catalogs = query_as!(
            WebhookEventCatalog,
            r#"
            SELECT deployment_id as "deployment_id!",
                   slug as "slug!",
                   name as "name!",
                   description,
                   events as "events!",
                   created_at as "created_at!",
                   updated_at as "updated_at!"
            FROM webhook_event_catalogs
            WHERE deployment_id = $1
            ORDER BY name ASC
            "#,
            self.deployment_id
        )
        .fetch_all(executor)
        .await?;

        Ok(catalogs)
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

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ModelWebhookEndpoint>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
                .fetch_all(executor)
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
                .fetch_all(executor)
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
                .fetch_all(executor)
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
                .fetch_all(executor)
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

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<WebhookEndpoint>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let limit = self.limit.unwrap_or(100) as i64;
        let offset = self.offset.unwrap_or(0) as i64;

        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
            r#"
            SELECT
                e.id, e.deployment_id, e.app_slug, e.url, e.description, e.headers,
                e.max_retries, e.timeout_seconds, e.is_active, e.failure_count, e.last_failure_at,
                e.auto_disabled, e.auto_disabled_at, e.rate_limit_config, e.created_at, e.updated_at,
                COALESCE(
                    (
                        SELECT json_agg(
                            json_build_object(
                                'event_name', s.event_name,
                                'filter_rules', s.filter_rules
                            )
                            ORDER BY s.event_name
                        )
                        FROM webhook_endpoint_subscriptions s
                        WHERE s.endpoint_id = e.id AND s.deployment_id = e.deployment_id
                    ),
                    '[]'::json
                ) AS subscriptions
            FROM webhook_endpoints e
            WHERE e.deployment_id = "#,
        );
        qb.push_bind(self.deployment_id);

        if let Some(app_slug) = &self.app_slug {
            qb.push(" AND e.app_slug = ");
            qb.push_bind(app_slug);
        }
        if !self.include_inactive {
            qb.push(" AND e.is_active = true");
        }

        qb.push(" ORDER BY e.created_at DESC LIMIT ");
        qb.push_bind(limit);
        qb.push(" OFFSET ");
        qb.push_bind(offset);

        let rows = qb.build().fetch_all(executor).await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let subscriptions_value: serde_json::Value = row.get("subscriptions");
            let subscriptions = parse_endpoint_subscriptions(subscriptions_value)?;
            out.push(map_webhook_endpoint_row(row, subscriptions));
        }

        Ok(out)
    }
}

#[derive(Debug, Deserialize)]
pub struct GetWebhookSubscriptionFilterRulesQuery {
    endpoint_id: i64,
    deployment_id: i64,
    app_slug: String,
    event_name: String,
}

impl GetWebhookSubscriptionFilterRulesQuery {
    pub fn new(endpoint_id: i64, deployment_id: i64, app_slug: String, event_name: String) -> Self {
        Self {
            endpoint_id,
            deployment_id,
            app_slug,
            event_name,
        }
    }

    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Option<serde_json::Value>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = query!(
            r#"
            SELECT filter_rules
            FROM webhook_endpoint_subscriptions
            WHERE endpoint_id = $1 AND deployment_id = $2 AND app_slug = $3 AND event_name = $4
            "#,
            self.endpoint_id,
            self.deployment_id,
            self.app_slug,
            self.event_name
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.and_then(|r| r.filter_rules))
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<WebhookApp>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
        .fetch_optional(executor)
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

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<models::webhook::WebhookEventDefinition>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            SELECT wa.event_catalog_slug, wec.events
            FROM webhook_apps wa
            LEFT JOIN webhook_event_catalogs wec
              ON wec.deployment_id = wa.deployment_id
             AND wec.slug = wa.event_catalog_slug
            WHERE wa.deployment_id = $1
              AND wa.app_slug = $2
            "#,
        )
        .bind(self.deployment_id)
        .bind(&self.app_slug)
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook app not found".to_string()))?;

        let catalog_slug: Option<String> = row
            .try_get("event_catalog_slug")
            .map_err(|e| AppError::Internal(format!("Invalid event_catalog_slug field: {}", e)))?;
        if catalog_slug.is_none() {
            return Ok(Vec::new());
        }

        let events_value: Option<serde_json::Value> = row
            .try_get("events")
            .map_err(|e| AppError::Internal(format!("Invalid events field: {}", e)))?;
        let events_value = events_value
            .ok_or_else(|| AppError::NotFound("Event catalog not found".to_string()))?;

        serde_json::from_value(events_value)
            .map_err(|e| AppError::Internal(format!("Invalid catalog events format: {}", e)))
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

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<dto::clickhouse::webhook::WebhookLog, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| AppError::NotFound("Pending delivery not found".to_string()))?;

        let payload_json = row
            .payload
            .map(|p| serde_json::to_string(&p))
            .transpose()
            .map_err(|e| AppError::Internal(format!("Failed to serialize webhook payload: {}", e)))?;
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
