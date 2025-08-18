use serde::{Deserialize, Serialize};
use sqlx::{query, query_as};

use common::error::AppError;
use common::state::AppState;
use models::webhook::{WebhookApp, WebhookEndpoint};

use super::Query;

#[derive(Debug, Deserialize)]
pub struct GetWebhookAppsQuery {
    deployment_id: i64,
    include_inactive: bool,
}

impl GetWebhookAppsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            include_inactive: false,
        }
    }

    pub fn with_inactive(mut self, include: bool) -> Self {
        self.include_inactive = include;
        self
    }
}

impl Query for GetWebhookAppsQuery {
    type Output = Vec<WebhookApp>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let apps = if self.include_inactive {
            query_as!(
                WebhookApp,
                r#"
                SELECT deployment_id as "deployment_id!",
                       name as "name!",
                       description,
                       signing_secret as "signing_secret!",
                       is_active as "is_active!",
                       created_at as "created_at!",
                       updated_at as "updated_at!"
                FROM webhook_apps
                WHERE deployment_id = $1
                ORDER BY created_at DESC
                "#,
                self.deployment_id
            )
            .fetch_all(&app_state.db_pool)
            .await?
        } else {
            query_as!(
                WebhookApp,
                r#"
                SELECT deployment_id as "deployment_id!",
                       name as "name!",
                       description,
                       signing_secret as "signing_secret!",
                       is_active as "is_active!",
                       created_at as "created_at!",
                       updated_at as "updated_at!"
                FROM webhook_apps
                WHERE deployment_id = $1 AND is_active = true
                ORDER BY created_at DESC
                "#,
                self.deployment_id
            )
            .fetch_all(&app_state.db_pool)
            .await?
        };

        Ok(apps)
    }
}

#[derive(Debug, Deserialize)]
pub struct GetWebhookEndpointsQuery {
    deployment_id: i64,
    app_name: Option<String>,
    include_inactive: bool,
}

impl GetWebhookEndpointsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            app_name: None,
            include_inactive: false,
        }
    }

    pub fn for_app(mut self, app_name: String) -> Self {
        self.app_name = Some(app_name);
        self
    }

    pub fn with_inactive(mut self, include: bool) -> Self {
        self.include_inactive = include;
        self
    }
}

impl Query for GetWebhookEndpointsQuery {
    type Output = Vec<WebhookEndpoint>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let endpoints = match (&self.app_name, self.include_inactive) {
            (Some(app_name), true) => {
                query_as!(
                    WebhookEndpoint,
                    r#"
                    SELECT e.id as "id!", e.deployment_id as "deployment_id!", e.app_name as "app_name!",
                           e.url as "url!", e.description, e.headers, e.signing_secret,
                           e.max_retries as "max_retries!", e.timeout_seconds as "timeout_seconds!", e.is_active as "is_active!",
                           e.failure_count as "failure_count!", e.last_failure_at, e.auto_disabled as "auto_disabled!", e.auto_disabled_at,
                           e.created_at as "created_at!", e.updated_at as "updated_at!"
                    FROM webhook_endpoints e
                    WHERE e.deployment_id = $1 AND e.app_name = $2
                    ORDER BY e.created_at DESC
                    "#,
                    self.deployment_id,
                    app_name
                )
                .fetch_all(&app_state.db_pool)
                .await?
            }
            (Some(app_name), false) => {
                query_as!(
                    WebhookEndpoint,
                    r#"
                    SELECT e.id as "id!", e.deployment_id as "deployment_id!", e.app_name as "app_name!",
                           e.url as "url!", e.description, e.headers, e.signing_secret,
                           e.max_retries as "max_retries!", e.timeout_seconds as "timeout_seconds!", e.is_active as "is_active!",
                           e.failure_count as "failure_count!", e.last_failure_at, e.auto_disabled as "auto_disabled!", e.auto_disabled_at,
                           e.created_at as "created_at!", e.updated_at as "updated_at!"
                    FROM webhook_endpoints e
                    WHERE e.deployment_id = $1 AND e.app_name = $2 AND e.is_active = true
                    ORDER BY e.created_at DESC
                    "#,
                    self.deployment_id,
                    app_name
                )
                .fetch_all(&app_state.db_pool)
                .await?
            }
            (None, true) => {
                query_as!(
                    WebhookEndpoint,
                    r#"
                    SELECT e.id as "id!", e.deployment_id as "deployment_id!", e.app_name as "app_name!",
                           e.url as "url!", e.description, e.headers, e.signing_secret,
                           e.max_retries as "max_retries!", e.timeout_seconds as "timeout_seconds!", e.is_active as "is_active!",
                           e.failure_count as "failure_count!", e.last_failure_at, e.auto_disabled as "auto_disabled!", e.auto_disabled_at,
                           e.created_at as "created_at!", e.updated_at as "updated_at!"
                    FROM webhook_endpoints e
                    WHERE e.deployment_id = $1
                    ORDER BY e.created_at DESC
                    "#,
                    self.deployment_id
                )
                .fetch_all(&app_state.db_pool)
                .await?
            }
            (None, false) => {
                query_as!(
                    WebhookEndpoint,
                    r#"
                    SELECT e.id as "id!", e.deployment_id as "deployment_id!", e.app_name as "app_name!",
                           e.url as "url!", e.description, e.headers, e.signing_secret,
                           e.max_retries as "max_retries!", e.timeout_seconds as "timeout_seconds!", e.is_active as "is_active!",
                           e.failure_count as "failure_count!", e.last_failure_at, e.auto_disabled as "auto_disabled!", e.auto_disabled_at,
                           e.created_at as "created_at!", e.updated_at as "updated_at!"
                    FROM webhook_endpoints e
                    WHERE e.deployment_id = $1 AND e.is_active = true
                    ORDER BY e.created_at DESC
                    "#,
                    self.deployment_id
                )
                .fetch_all(&app_state.db_pool)
                .await?
            }
        };

        Ok(endpoints)
    }
}

#[derive(Debug, Serialize)]
pub struct WebhookDeliveryInfo {
    pub id: i64,
    pub endpoint_id: i64,
    pub event_name: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub next_retry_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct GetWebhookDeliveryStatusQuery {
    delivery_id: i64,
    deployment_id: i64,
}

impl GetWebhookDeliveryStatusQuery {
    pub fn new(delivery_id: i64, deployment_id: i64) -> Self {
        Self {
            delivery_id,
            deployment_id,
        }
    }
}

impl Query for GetWebhookDeliveryStatusQuery {
    type Output = WebhookDeliveryInfo;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let delivery = query!(
            r#"
            SELECT d.id as "id!",
                   d.endpoint_id as "endpoint_id!",
                   d.event_name as "event_name!",
                   d.attempts as "attempts!",
                   d.max_attempts as "max_attempts!",
                   d.next_retry_at,
                   d.created_at as "created_at!"
            FROM active_webhook_deliveries d
            WHERE d.id = $1 AND d.deployment_id = $2
            "#,
            self.delivery_id,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        match delivery {
            Some(d) => Ok(WebhookDeliveryInfo {
                id: d.id,
                endpoint_id: d.endpoint_id,
                event_name: d.event_name,
                attempts: d.attempts,
                max_attempts: d.max_attempts,
                next_retry_at: d.next_retry_at,
                created_at: d.created_at,
            }),
            None => Err(AppError::NotFound("Delivery not found".to_string())),
        }
    }
}

// Query for getting webhook app by name
#[derive(Debug, Deserialize)]
pub struct GetWebhookAppByNameQuery {
    deployment_id: i64,
    app_name: String,
}

impl GetWebhookAppByNameQuery {
    pub fn new(deployment_id: i64, app_name: String) -> Self {
        Self {
            deployment_id,
            app_name,
        }
    }
}

impl Query for GetWebhookAppByNameQuery {
    type Output = Option<WebhookApp>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let app = query_as!(
            WebhookApp,
            r#"
            SELECT deployment_id as "deployment_id!",
                   name as "name!",
                   description,
                   signing_secret as "signing_secret!",
                   is_active as "is_active!",
                   created_at as "created_at!",
                   updated_at as "updated_at!"
            FROM webhook_apps
            WHERE deployment_id = $1 AND name = $2
            "#,
            self.deployment_id,
            self.app_name
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(app)
    }
}

// Query for getting webhook events for an app
#[derive(Debug)]
pub struct GetWebhookEventsQuery {
    deployment_id: i64,
    app_name: String,
}

impl GetWebhookEventsQuery {
    pub fn new(deployment_id: i64, app_name: String) -> Self {
        Self {
            deployment_id,
            app_name,
        }
    }
}

impl Query for GetWebhookEventsQuery {
    type Output = Vec<models::webhook::WebhookAppEvent>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let events = query_as!(
            models::webhook::WebhookAppEvent,
            r#"
            SELECT
                deployment_id as "deployment_id!",
                app_name as "app_name!",
                event_name as "event_name!",
                description,
                schema,
                created_at as "created_at!"
            FROM webhook_app_events
            WHERE deployment_id = $1 AND app_name = $2
            ORDER BY event_name
            "#,
            self.deployment_id,
            self.app_name
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        Ok(events)
    }
}

// Query for getting webhook stats from ClickHouse
#[derive(Debug)]
pub struct GetWebhookStatsQuery {
    deployment_id: i64,
    app_name: String,
}

impl GetWebhookStatsQuery {
    pub fn new(deployment_id: i64, app_name: String) -> Self {
        Self {
            deployment_id,
            app_name,
        }
    }
}

impl Query for GetWebhookStatsQuery {
    type Output = dto::json::WebhookStats;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        use dto::json::WebhookStats;

        // Get active endpoints count from PostgreSQL
        let active_endpoints = query!(
            "SELECT COUNT(*) as count FROM webhook_endpoints WHERE deployment_id = $1 AND app_name = $2 AND is_active = true",
            self.deployment_id,
            &self.app_name
        )
        .fetch_one(&app_state.db_pool)
        .await?
        .count
        .unwrap_or(0);

        // Get delivery stats from ClickHouse
        let delivery_stats = app_state
            .clickhouse_service
            .get_webhook_delivery_stats(
                self.deployment_id,
                Some(self.app_name.clone()),
                None,
                chrono::Utc::now() - chrono::Duration::days(30),
                chrono::Utc::now(),
            )
            .await?;

        // Calculate stats from ClickHouse data
        let total = delivery_stats.total_deliveries;
        let success = delivery_stats.successful_deliveries;
        let success_rate = if total > 0 {
            (success as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        // Get failed deliveries in last 24 hours
        let recent_stats = app_state
            .clickhouse_service
            .get_webhook_delivery_stats(
                self.deployment_id,
                Some(self.app_name.clone()),
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
