use crate::error::AppError;
use chrono::{DateTime, NaiveDateTime, Utc};
use clickhouse::{Client, Row};
use dto::clickhouse::ApiKeyVerificationEvent;
use dto::clickhouse::webhook::*;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{debug, error, info};

#[derive(Clone)]
pub struct ClickHouseService {
    pub client: Client,
}

#[derive(Debug, Clone)]
enum ClickHouseBind {
    I64(i64),
    String(String),
    DateTime(DateTime<Utc>),
}

fn bind_clickhouse_query(
    mut query: clickhouse::query::Query,
    binds: &[ClickHouseBind],
) -> clickhouse::query::Query {
    for bind in binds {
        query = match bind {
            ClickHouseBind::I64(v) => query.bind(*v),
            ClickHouseBind::String(v) => query.bind(v.clone()),
            ClickHouseBind::DateTime(v) => query.bind(*v),
        };
    }
    query
}

#[derive(Serialize, Deserialize, Row)]
pub struct UserEvent {
    pub deployment_id: i64,
    pub user_id: Option<i64>,
    pub event_type: String,
    pub user_name: Option<String>,
    pub user_identifier: Option<String>,
    pub auth_method: Option<String>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    pub timestamp: DateTime<Utc>,
    pub ip_address: Option<String>,
}

#[derive(Serialize, Deserialize, Row)]
struct CountResult {
    count: u64,
}

#[derive(Serialize, Deserialize, Row)]
pub struct AnalyticsStatsResult {
    pub unique_signins: u64,
    pub signups: u64,
    pub organizations_created: u64,
    pub workspaces_created: u64,
    pub previous_signins: u64,
    pub previous_signups: u64,
    pub previous_orgs: u64,
    pub previous_workspaces: u64,
    pub total_signups: u64,
    // Recent signups - Tuple from single subquery: (names, emails, methods, timestamps)
    pub recent_signups: (Vec<String>, Vec<String>, Vec<String>, Vec<String>),
    // Recent signins - Tuple from single subquery: (names, emails, methods, timestamps)
    pub recent_signins: (Vec<String>, Vec<String>, Vec<String>, Vec<String>),
    // Daily metrics - Tuple from single subquery: (days, signins, signups)
    pub daily_metrics: (Vec<String>, Vec<u64>, Vec<u64>),
}

impl AnalyticsStatsResult {
    fn parse_clickhouse_timestamp(timestamp: &str) -> Option<DateTime<Utc>> {
        NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%d %H:%M:%S%.f")
            .ok()
            .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
    }

    /// Convert to recent signups
    pub fn get_recent_signups(&self) -> Vec<RecentSignup> {
        self.recent_signups
            .0
            .iter()
            .zip(&self.recent_signups.1)
            .zip(&self.recent_signups.2)
            .zip(&self.recent_signups.3)
            .filter_map(|(((name, email), method), date)| {
                Self::parse_clickhouse_timestamp(date).map(|parsed_date| RecentSignup {
                    name: Some(name.clone()),
                    email: Some(email.clone()),
                    method: Some(method.clone()),
                    date: parsed_date,
                })
            })
            .collect()
    }

    /// Convert to recent signins
    pub fn get_recent_signins(&self) -> Vec<RecentSignup> {
        self.recent_signins
            .0
            .iter()
            .zip(&self.recent_signins.1)
            .zip(&self.recent_signins.2)
            .zip(&self.recent_signins.3)
            .filter_map(|(((name, email), method), date)| {
                Self::parse_clickhouse_timestamp(date).map(|parsed_date| RecentSignup {
                    name: Some(name.clone()),
                    email: Some(email.clone()),
                    method: Some(method.clone()),
                    date: parsed_date,
                })
            })
            .collect()
    }

    pub fn get_daily_metrics(&self) -> Vec<(String, u64, u64)> {
        self.daily_metrics
            .0
            .iter()
            .zip(&self.daily_metrics.1)
            .zip(&self.daily_metrics.2)
            .map(|((day, signins), signups)| (day.clone(), *signins, *signups))
            .collect()
    }
}

#[derive(Serialize, Deserialize)]
pub struct RecentSignup {
    pub name: Option<String>,
    pub email: Option<String>,
    pub method: Option<String>,
    pub date: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Row)]
struct RecentSignupRow {
    user_name: Option<String>,
    user_identifier: Option<String>,
    auth_method: Option<String>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    timestamp: DateTime<Utc>,
}

impl ClickHouseService {
    pub fn new(url: String, password: String) -> Result<Self, AppError> {
        let url = if url.starts_with("https://") {
            url
        } else {
            format!("https://{}", url)
        };

        let client = Client::default()
            .with_url(url)
            .with_user("wacht")
            .with_database("wacht")
            .with_password(password);

        Ok(Self { client })
    }

    pub async fn get_total_signups(&self, deployment_id: i64) -> Result<i64, AppError> {
        let query = "SELECT count(DISTINCT user_id) as count FROM user_events WHERE deployment_id = ? AND event_type = 'signup' AND user_id IS NOT NULL";

        debug!(deployment_id, "Executing get_total_signups query");
        let start = Instant::now();

        let result = self
            .client
            .query(query)
            .bind(deployment_id)
            .fetch_one::<CountResult>()
            .await
            .map_err(|e| {
                error!(error = ?e, deployment_id, "ClickHouse query failed for get_total_signups");
                e
            })?;

        info!(
            deployment_id,
            "get_total_signups query took: {:?}",
            start.elapsed()
        );
        Ok(result.count as i64)
    }

    pub async fn get_unique_signins(
        &self,
        deployment_id: i64,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<i64, AppError> {
        let from_str = from.format("%Y-%m-%d %H:%M:%S").to_string();
        let to_str = to.format("%Y-%m-%d %H:%M:%S").to_string();

        debug!(deployment_id, %from_str, %to_str, "Executing get_unique_signins query");
        let start = Instant::now();

        let query = "SELECT count(DISTINCT user_id) as count FROM user_events WHERE deployment_id = ? AND event_type = 'signin' AND timestamp >= ? AND timestamp <= ? AND user_id IS NOT NULL";

        let result = self
            .client
            .query(query)
            .bind(deployment_id)
            .bind(&from_str)
            .bind(&to_str)
            .fetch_one::<CountResult>()
            .await
            .map_err(|e| {
                error!(error = ?e, deployment_id, "ClickHouse query failed for get_unique_signins");
                e
            })?;

        info!(
            deployment_id,
            "get_unique_signins query took: {:?}",
            start.elapsed()
        );
        Ok(result.count as i64)
    }

    pub async fn get_signups(
        &self,
        deployment_id: i64,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<i64, AppError> {
        let from_str = from.format("%Y-%m-%d %H:%M:%S").to_string();
        let to_str = to.format("%Y-%m-%d %H:%M:%S").to_string();

        debug!(deployment_id, %from_str, %to_str, "Executing get_signups query");
        let start = Instant::now();

        let query = "SELECT count(*) as count FROM user_events WHERE deployment_id = ? AND event_type = 'signup' AND timestamp >= ? AND timestamp <= ?";

        let result = self
            .client
            .query(query)
            .bind(deployment_id)
            .bind(&from_str)
            .bind(&to_str)
            .fetch_one::<CountResult>()
            .await
            .map_err(|e| {
                error!(error = ?e, deployment_id, "ClickHouse query failed for get_signups");
                e
            })?;

        info!(
            deployment_id,
            "get_signups query took: {:?}",
            start.elapsed()
        );
        Ok(result.count as i64)
    }

    pub async fn get_organizations_created(
        &self,
        deployment_id: i64,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<i64, AppError> {
        let from_str = from.format("%Y-%m-%d %H:%M:%S").to_string();
        let to_str = to.format("%Y-%m-%d %H:%M:%S").to_string();

        debug!(deployment_id, %from_str, %to_str, "Executing get_organizations_created query");

        let query = "SELECT count(*) as count FROM user_events WHERE deployment_id = ? AND event_type = 'organization_created' AND timestamp >= ? AND timestamp <= ?";

        let result = self
            .client
            .query(query)
            .bind(deployment_id)
            .bind(&from_str)
            .bind(&to_str)
            .fetch_one::<CountResult>()
            .await
            .map_err(|e| {
                error!(error = ?e, deployment_id, "ClickHouse query failed for get_organizations_created");
                e
            })?;

        Ok(result.count as i64)
    }

    pub async fn get_workspaces_created(
        &self,
        deployment_id: i64,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<i64, AppError> {
        let from_str = from.format("%Y-%m-%d %H:%M:%S").to_string();
        let to_str = to.format("%Y-%m-%d %H:%M:%S").to_string();

        debug!(deployment_id, %from_str, %to_str, "Executing get_workspaces_created query");

        let query = "SELECT count(*) as count FROM user_events WHERE deployment_id = ? AND event_type = 'workspace_created' AND timestamp >= ? AND timestamp <= ?";

        let result = self
            .client
            .query(query)
            .bind(deployment_id)
            .bind(&from_str)
            .bind(&to_str)
            .fetch_one::<CountResult>()
            .await
            .map_err(|e| {
                error!(error = ?e, deployment_id, "ClickHouse query failed for get_workspaces_created");
                e
            })?;

        Ok(result.count as i64)
    }

    pub async fn get_recent_signups(
        &self,
        deployment_id: i64,
        limit: i32,
    ) -> Result<Vec<RecentSignup>, AppError> {
        debug!(deployment_id, limit, "Executing get_recent_signups query");

        let query = "SELECT user_name, user_identifier, auth_method, timestamp FROM user_events WHERE deployment_id = ? AND event_type = 'signup' ORDER BY timestamp DESC LIMIT ?";

        let rows = self
            .client
            .query(query)
            .bind(deployment_id)
            .bind(limit)
            .fetch_all::<RecentSignupRow>()
            .await
            .map_err(|e| {
                error!(error = ?e, deployment_id, limit, "ClickHouse query failed for get_recent_signups");
                e
            })?;

        Ok(rows
            .into_iter()
            .map(|row| RecentSignup {
                name: row.user_name,
                email: row.user_identifier,
                method: row.auth_method,
                date: row.timestamp,
            })
            .collect())
    }

    pub async fn get_recent_signins(
        &self,
        deployment_id: i64,
        limit: i32,
    ) -> Result<Vec<RecentSignup>, AppError> {
        debug!(deployment_id, limit, "Executing get_recent_signins query");

        let query = "SELECT user_name, user_identifier, auth_method, timestamp FROM user_events WHERE deployment_id = ? AND event_type = 'signin' ORDER BY timestamp DESC LIMIT ?";

        let rows = self
            .client
            .query(query)
            .bind(deployment_id)
            .bind(limit)
            .fetch_all::<RecentSignupRow>()
            .await
            .map_err(|e| {
                error!(error = ?e, deployment_id, limit, "ClickHouse query failed for get_recent_signins");
                e
            })?;

        Ok(rows
            .into_iter()
            .map(|row| RecentSignup {
                name: row.user_name,
                email: row.user_identifier,
                method: row.auth_method,
                date: row.timestamp,
            })
            .collect())
    }

    /// Get all analytics stats in a single query
    pub async fn get_analytics_stats(
        &self,
        deployment_id: i64,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        previous_from: DateTime<Utc>,
        previous_to: DateTime<Utc>,
    ) -> Result<AnalyticsStatsResult, AppError> {
        let from_ts = from.timestamp();
        let to_ts = to.timestamp();
        let prev_from_ts = previous_from.timestamp();
        let prev_to_ts = previous_to.timestamp();

        info!(
            deployment_id,
            from_ts,
            to_ts,
            prev_from_ts,
            prev_to_ts,
            "Executing get_analytics_stats combined query"
        );
        let start = Instant::now();

        let query = r#"
            SELECT
                count(DISTINCT CASE WHEN event_type = 'signin' AND user_id IS NOT NULL AND timestamp >= fromUnixTimestamp64Milli(?*1000) AND timestamp <= fromUnixTimestamp64Milli(?*1000) THEN user_id END) as unique_signins,
                count(CASE WHEN event_type = 'signup' AND timestamp >= fromUnixTimestamp64Milli(?*1000) AND timestamp <= fromUnixTimestamp64Milli(?*1000) THEN 1 END) as signups,
                count(CASE WHEN event_type = 'organization_created' AND timestamp >= fromUnixTimestamp64Milli(?*1000) AND timestamp <= fromUnixTimestamp64Milli(?*1000) THEN 1 END) as organizations_created,
                count(CASE WHEN event_type = 'workspace_created' AND timestamp >= fromUnixTimestamp64Milli(?*1000) AND timestamp <= fromUnixTimestamp64Milli(?*1000) THEN 1 END) as workspaces_created,
                count(DISTINCT CASE WHEN event_type = 'signin' AND user_id IS NOT NULL AND timestamp >= fromUnixTimestamp64Milli(?*1000) AND timestamp <= fromUnixTimestamp64Milli(?*1000) THEN user_id END) as previous_signins,
                count(CASE WHEN event_type = 'signup' AND timestamp >= fromUnixTimestamp64Milli(?*1000) AND timestamp <= fromUnixTimestamp64Milli(?*1000) THEN 1 END) as previous_signups,
                count(CASE WHEN event_type = 'organization_created' AND timestamp >= fromUnixTimestamp64Milli(?*1000) AND timestamp <= fromUnixTimestamp64Milli(?*1000) THEN 1 END) as previous_orgs,
                count(CASE WHEN event_type = 'workspace_created' AND timestamp >= fromUnixTimestamp64Milli(?*1000) AND timestamp <= fromUnixTimestamp64Milli(?*1000) THEN 1 END) as previous_workspaces,
                count(DISTINCT CASE WHEN event_type = 'signup' AND user_id IS NOT NULL THEN user_id END) as total_signups,
                (SELECT groupArray(user_name), groupArray(user_identifier), groupArray(auth_method), groupArray(formatDateTime(timestamp, '%Y-%m-%d %H:%i:%S.%f'))
                 FROM (SELECT user_name, user_identifier, auth_method, timestamp
                       FROM user_events
                       WHERE deployment_id = ? AND event_type = 'signup'
                       ORDER BY timestamp DESC
                       LIMIT 10)) as recent_signups,
                (SELECT groupArray(user_name), groupArray(user_identifier), groupArray(auth_method), groupArray(formatDateTime(timestamp, '%Y-%m-%d %H:%i:%S.%f'))
                 FROM (SELECT user_name, user_identifier, auth_method, timestamp
                       FROM user_events
                       WHERE deployment_id = ? AND event_type = 'signin'
                       ORDER BY timestamp DESC
                       LIMIT 10)) as recent_signins,
                (SELECT groupArray(day), groupArray(signins), groupArray(signups)
                 FROM (
                       SELECT
                           formatDateTime(toDate(timestamp), '%Y-%m-%d') as day,
                           countDistinctIf(user_id, event_type = 'signin' AND user_id IS NOT NULL) as signins,
                           countIf(event_type = 'signup') as signups
                       FROM user_events
                       WHERE deployment_id = ?
                         AND timestamp >= fromUnixTimestamp64Milli(?*1000)
                         AND timestamp <= fromUnixTimestamp64Milli(?*1000)
                         AND event_type IN ('signin', 'signup')
                       GROUP BY toDate(timestamp)
                       ORDER BY toDate(timestamp) ASC
                 )) as daily_metrics
            FROM user_events
            WHERE deployment_id = ?
                AND ((timestamp >= fromUnixTimestamp64Milli(?*1000) AND timestamp <= fromUnixTimestamp64Milli(?*1000)) OR (timestamp >= fromUnixTimestamp64Milli(?*1000) AND timestamp <= fromUnixTimestamp64Milli(?*1000)))
        "#;

        let result = self
            .client
            .query(query)
            .bind(from_ts)
            .bind(to_ts)
            .bind(from_ts)
            .bind(to_ts)
            .bind(from_ts)
            .bind(to_ts)
            .bind(from_ts)
            .bind(to_ts)
            .bind(prev_from_ts)
            .bind(prev_to_ts)
            .bind(prev_from_ts)
            .bind(prev_to_ts)
            .bind(prev_from_ts)
            .bind(prev_to_ts)
            .bind(prev_from_ts)
            .bind(prev_to_ts)
            .bind(deployment_id)
            .bind(deployment_id)
            .bind(deployment_id)
            .bind(from_ts)
            .bind(to_ts)
            .bind(deployment_id)
            .bind(from_ts)
            .bind(to_ts)
            .bind(prev_from_ts)
            .bind(prev_to_ts)
            .fetch_one::<AnalyticsStatsResult>()
            .await
            .map_err(|e| {
                error!(
                    error = ?e,
                    error_msg = %e,
                    deployment_id,
                    from_ts,
                    to_ts,
                    "ClickHouse query failed for get_analytics_stats"
                );
                e
            })?;

        info!(
            deployment_id,
            "get_analytics_stats combined query took: {:?}",
            start.elapsed()
        );

        Ok(result)
    }

    pub async fn insert_user_event(&self, event: &UserEvent) -> Result<(), AppError> {
        let mut insert = self.client.insert::<UserEvent>("user_events").await?;
        insert.write(event).await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn insert_api_audit_log(
        &self,
        event: &ApiKeyVerificationEvent,
    ) -> Result<(), AppError> {
        let mut insert = self
            .client
            .insert::<ApiKeyVerificationEvent>("api_audit_logs")
            .await?;
        insert.write(event).await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn insert_webhook_log(&self, log: &WebhookLog) -> Result<(), AppError> {
        let mut full_insert = self.client.insert::<WebhookLog>("webhook_logs_full").await?;
        full_insert.write(log).await?;
        full_insert.end().await?;

        let log_light = WebhookLogLight {
            deployment_id: log.deployment_id,
            delivery_id: log.delivery_id,
            app_slug: log.app_slug.clone(),
            endpoint_id: log.endpoint_id,
            event_name: log.event_name.clone(),
            status: log.status.clone(),
            http_status_code: log.http_status_code,
            response_time_ms: log.response_time_ms,
            attempt_number: log.attempt_number,
            max_attempts: log.max_attempts,
            payload_size_bytes: log.payload_size_bytes,
            timestamp: log.timestamp,
        };

        let mut light_insert = self
            .client
            .insert::<WebhookLogLight>("webhook_logs_light")
            .await?;
        light_insert.write(&log_light).await?;
        light_insert.end().await?;
        Ok(())
    }

    pub async fn batch_insert_webhook_logs(&self, logs: &[WebhookLog]) -> Result<(), AppError> {
        for log in logs {
            self.insert_webhook_log(log).await?;
        }
        Ok(())
    }

    pub async fn get_webhook_delivery_stats(
        &self,
        deployment_id: i64,
        app_slug: Option<String>,
        endpoint_id: Option<i64>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<WebhookDeliveryStatsRow, AppError> {
        let mut conditions = vec!["deployment_id = ?".to_string()];
        let mut bindings: Vec<String> = vec![deployment_id.to_string()];

        if let Some(ref slug) = app_slug {
            conditions.push("app_slug = ?".to_string());
            bindings.push(slug.clone());
        }

        if let Some(endpoint_id) = endpoint_id {
            conditions.push("endpoint_id = ?".to_string());
            bindings.push(endpoint_id.to_string());
        }

        conditions.push("timestamp >= ?".to_string());
        bindings.push(from.format("%Y-%m-%d %H:%M:%S%.6f").to_string());
        conditions.push("timestamp <= ?".to_string());
        bindings.push(to.format("%Y-%m-%d %H:%M:%S%.6f").to_string());

        let query = format!(
            r#"
                SELECT
                    CAST(count() AS Int64) as total_deliveries,
                    CAST(countIf(status = 'success') AS Int64) as successful_deliveries,
                    CAST(countIf(status IN ('failed', 'permanently_failed')) AS Int64) as failed_deliveries,
                    CAST(countIf(status = 'filtered') AS Int64) as filtered_deliveries,
                    CAST(avgOrNull(response_time_ms) AS Nullable(Float64)) as avg_response_time_ms,
                    CAST(quantileOrNull(0.5)(response_time_ms) AS Nullable(Float64)) as p50_response_time_ms,
                    CAST(quantileOrNull(0.95)(response_time_ms) AS Nullable(Float64)) as p95_response_time_ms,
                    CAST(quantileOrNull(0.99)(response_time_ms) AS Nullable(Float64)) as p99_response_time_ms,
                    CAST(count(DISTINCT event_name) AS Int64) as total_events
                FROM webhook_logs_full
                WHERE {}
            "#,
            conditions.join(" AND ")
        );

        let mut query_builder = self.client.query(&query);
        for binding in bindings {
            query_builder = query_builder.bind(binding);
        }

        let result = query_builder.fetch_one::<WebhookDeliveryStatsRow>().await?;

        Ok(result)
    }

    pub async fn get_webhook_event_distribution(
        &self,
        deployment_id: i64,
        app_slug: Option<String>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<WebhookEventDistribution>, AppError> {
        let query = if app_slug.is_some() {
            format!(
                r#"
                SELECT
                    event_name,
                    CAST(count() AS Int64) as count
                FROM webhook_logs_full
                WHERE deployment_id = ? AND app_slug = ? AND timestamp >= ? AND timestamp <= ?
                GROUP BY event_name
                ORDER BY count DESC
                LIMIT {}
            "#,
                limit
            )
        } else {
            format!(
                r#"
                SELECT
                    event_name,
                    CAST(count() AS Int64) as count
                FROM webhook_logs_full
                WHERE deployment_id = ? AND timestamp >= ? AND timestamp <= ?
                GROUP BY event_name
                ORDER BY count DESC
                LIMIT {}
            "#,
                limit
            )
        };

        let rows = if let Some(ref slug) = app_slug {
            self.client
                .query(&query)
                .bind(deployment_id)
                .bind(slug.clone())
                .bind(from.format("%Y-%m-%d %H:%M:%S").to_string())
                .bind(to.format("%Y-%m-%d %H:%M:%S").to_string())
                .fetch_all::<WebhookEventDistributionRow>()
                .await?
        } else {
            self.client
                .query(&query)
                .bind(deployment_id)
                .bind(from.format("%Y-%m-%d %H:%M:%S").to_string())
                .bind(to.format("%Y-%m-%d %H:%M:%S").to_string())
                .fetch_all::<WebhookEventDistributionRow>()
                .await?
        };

        Ok(rows
            .into_iter()
            .map(|row| WebhookEventDistribution {
                event_name: row.event_name,
                count: row.count,
            })
            .collect())
    }

    pub async fn get_webhook_endpoint_performance(
        &self,
        deployment_id: i64,
        endpoint_id: i64,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<WebhookEndpointPerformanceResponse, AppError> {
        let query = r#"
            SELECT
                toString(endpoint_id) as endpoint_url,
                CAST(count() AS Int64) as total_attempts,
                CAST(countIf(status = 'success') AS Int64) as successful_attempts,
                avg(response_time_ms) as avg_response_time,
                quantile(0.5)(response_time_ms) as p50_response_time,
                quantile(0.95)(response_time_ms) as p95_response_time,
                quantile(0.99)(response_time_ms) as p99_response_time,
                max(response_time_ms) as max_response_time,
                min(response_time_ms) as min_response_time
            FROM webhook_logs_full
            WHERE deployment_id = ? AND endpoint_id = ? AND timestamp >= ? AND timestamp <= ?
            GROUP BY endpoint_id
        "#;

        let result = self
            .client
            .query(query)
            .bind(deployment_id)
            .bind(endpoint_id)
            .bind(from.format("%Y-%m-%d %H:%M:%S").to_string())
            .bind(to.format("%Y-%m-%d %H:%M:%S").to_string())
            .fetch_one::<WebhookEndpointPerformanceRow>()
            .await?;

        Ok(WebhookEndpointPerformanceResponse::from(result))
    }

    pub async fn get_webhook_failure_reasons(
        &self,
        deployment_id: i64,
        app_slug: Option<String>,
        endpoint_id: Option<i64>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<WebhookFailureReasonResponse>, AppError> {
        let mut where_conditions = vec!["deployment_id = ?".to_string()];
        let mut bindings: Vec<String> = vec![deployment_id.to_string()];

        if let Some(ref slug) = app_slug {
            where_conditions.push("app_slug = ?".to_string());
            bindings.push(slug.clone());
        }

        if let Some(endpoint_id) = endpoint_id {
            where_conditions.push("endpoint_id = ?".to_string());
            bindings.push(endpoint_id.to_string());
        }

        where_conditions.push("status IN ('failed', 'permanently_failed')".to_string());
        where_conditions.push("timestamp >= ?".to_string());
        bindings.push(from.format("%Y-%m-%d %H:%M:%S").to_string());
        where_conditions.push("timestamp <= ?".to_string());
        bindings.push(to.format("%Y-%m-%d %H:%M:%S").to_string());

        let query = format!(
            r#"
                SELECT
                    CASE
                        WHEN http_status_code >= 500 THEN 'Server Error (5xx)'
                        WHEN http_status_code >= 400 THEN 'Client Error (4xx)'
                        WHEN http_status_code = 0 THEN 'Connection Failed'
                        ELSE 'Unknown'
                    END as reason,
                    CAST(count() AS Int64) as count
                FROM webhook_logs_full
                WHERE {}
                GROUP BY reason
                ORDER BY count DESC
                LIMIT 10
            "#,
            where_conditions.join(" AND ")
        );

        let mut query_builder = self.client.query(&query);
        for binding in bindings {
            query_builder = query_builder.bind(binding);
        }

        let rows = query_builder.fetch_all::<WebhookFailureReasonRow>().await?;

        Ok(rows
            .into_iter()
            .map(|row| WebhookFailureReasonResponse {
                reason: row.reason,
                count: row.count,
            })
            .collect())
    }

    pub async fn get_app_endpoints_performance(
        &self,
        deployment_id: i64,
        app_slug: String,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<WebhookEndpointStatsResponse>, AppError> {
        let query = r#"
            SELECT
                endpoint_id,
                toString(endpoint_id) as endpoint_url,
                CAST(count() AS Int64) as total_attempts,
                CAST(countIf(status = 'success') AS Int64) as successful_attempts,
                CAST(countIf(status IN ('failed', 'permanently_failed')) AS Int64) as failed_attempts,
                avg(response_time_ms) as avg_response_time_ms
            FROM webhook_logs_full
            WHERE deployment_id = ? AND app_slug = ? AND timestamp >= ? AND timestamp <= ?
            GROUP BY endpoint_id
            ORDER BY total_attempts DESC
            LIMIT 20
        "#;

        let rows = self
            .client
            .query(query)
            .bind(deployment_id)
            .bind(app_slug)
            .bind(from.format("%Y-%m-%d %H:%M:%S").to_string())
            .bind(to.format("%Y-%m-%d %H:%M:%S").to_string())
            .fetch_all::<WebhookEndpointStatsRow>()
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| WebhookEndpointStatsResponse {
                endpoint_id: row.endpoint_id,
                endpoint_url: row.endpoint_url,
                total_attempts: row.total_attempts,
                successful_attempts: row.successful_attempts,
                failed_attempts: row.failed_attempts,
                avg_response_time_ms: row.avg_response_time_ms,
                success_rate: if row.total_attempts > 0 {
                    (row.successful_attempts as f64 / row.total_attempts as f64) * 100.0
                } else {
                    0.0
                },
            })
            .collect())
    }

    pub async fn get_webhook_timeseries(
        &self,
        deployment_id: i64,
        app_slug: Option<String>,
        endpoint_id: Option<i64>,
        interval: &models::webhook_analytics::TimeseriesInterval,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<WebhookTimeseriesResponse>, AppError> {
        let mut where_conditions = vec!["deployment_id = ?".to_string()];
        let mut bindings: Vec<String> = vec![deployment_id.to_string()];

        if let Some(ref slug) = app_slug {
            where_conditions.push("app_slug = ?".to_string());
            bindings.push(slug.clone());
        }

        if let Some(endpoint_id) = endpoint_id {
            where_conditions.push("endpoint_id = ?".to_string());
            bindings.push(endpoint_id.to_string());
        }

        where_conditions.push("timestamp >= ?".to_string());
        bindings.push(from.format("%Y-%m-%d %H:%M:%S").to_string());
        where_conditions.push("timestamp <= ?".to_string());
        bindings.push(to.format("%Y-%m-%d %H:%M:%S").to_string());

        let interval_fn = interval.to_clickhouse_interval();

        let query = format!(
            r#"
                SELECT
                    toDateTime64({}(timestamp), 6) as bucket,
                    toInt64(count()) as total_deliveries,
                    toInt64(countIf(status = 'success')) as successful_deliveries,
                    toInt64(countIf(status IN ('failed', 'permanently_failed'))) as failed_deliveries,
                    toInt64(countIf(status = 'filtered')) as filtered_deliveries,
                    avg(response_time_ms) as avg_response_time_ms
                FROM webhook_logs_full
                WHERE {}
                GROUP BY bucket
                ORDER BY bucket ASC
            "#,
            interval_fn,
            where_conditions.join(" AND ")
        );

        let mut query_builder = self.client.query(&query);
        for binding in &bindings {
            query_builder = query_builder.bind(binding.clone());
        }

        let delivery_rows = query_builder
            .fetch_all::<WebhookDeliveryTimeseriesRow>()
            .await?;

        // Consolidated table - events and deliveries are the same
        Ok(delivery_rows
            .into_iter()
            .map(|row| {
                let total_events = row.total_deliveries;
                let success_rate = if row.total_deliveries > 0 {
                    (row.successful_deliveries as f64 / row.total_deliveries as f64) * 100.0
                } else {
                    0.0
                };

                WebhookTimeseriesResponse {
                    timestamp: row.bucket,
                    total_events,
                    total_deliveries: row.total_deliveries,
                    successful_deliveries: row.successful_deliveries,
                    failed_deliveries: row.failed_deliveries,
                    filtered_deliveries: row.filtered_deliveries,
                    avg_response_time_ms: row.avg_response_time_ms,
                    success_rate,
                }
            })
            .collect())
    }

    pub async fn get_webhook_deliveries(
        &self,
        deployment_id: i64,
        app_slug: Option<String>,
        status: Option<&str>,
        event_name: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<WebhookDeliveryListLightRow>, AppError> {
        let mut query = String::from(
            "SELECT
                delivery_id,
                deployment_id,
                app_slug,
                endpoint_id,
                event_name,
                status,
                http_status_code,
                response_time_ms,
                attempt_number,
                max_attempts,
                payload_size_bytes,
                timestamp
            FROM webhook_logs_light
            WHERE deployment_id = ?",
        );
        let mut binds = vec![ClickHouseBind::I64(deployment_id)];

        if let Some(ref slug) = app_slug {
            query.push_str(" AND app_slug = ?");
            binds.push(ClickHouseBind::String(slug.clone()));
        }

        if let Some(status_val) = status {
            query.push_str(" AND status = ?");
            binds.push(ClickHouseBind::String(status_val.to_string()));
        }

        if let Some(event_name_val) = event_name {
            query.push_str(" AND event_name = ?");
            binds.push(ClickHouseBind::String(event_name_val.to_string()));
        }

        query.push_str(&format!(
            " ORDER BY timestamp DESC LIMIT {limit} OFFSET {offset}"
        ));

        tracing::info!("Executing ClickHouse query for deliveries");

        let rows = match bind_clickhouse_query(self.client.query(&query), &binds)
            .fetch_all::<WebhookDeliveryListLightRow>()
            .await
        {
            Ok(r) => {
                tracing::info!("ClickHouse query successful, fetched {} rows", r.len());
                r
            }
            Err(e) => {
                tracing::error!("ClickHouse query failed: {:?}", e);
                return Err(AppError::Internal(format!(
                    "ClickHouse query failed: {}",
                    e
                )));
            }
        };

        Ok(rows)
    }

    pub async fn get_webhook_delivery_details(
        &self,
        deployment_id: i64,
        delivery_id: i64,
    ) -> Result<WebhookLog, AppError> {
        tracing::info!(
            "Getting delivery details for deployment_id={}, delivery_id={}",
            deployment_id,
            delivery_id
        );

        let query = "SELECT
                deployment_id,
                delivery_id,
                app_slug,
                endpoint_id,
                endpoint_url,
                event_name,
                event_id,
                status,
                http_status_code,
                response_time_ms,
                attempt_number,
                max_attempts,
                error_message,
                filtered_reason,
                payload,
                payload_size_bytes,
                response_body,
                response_headers,
                filter_context,
                timestamp
            FROM webhook_logs_full
            WHERE deployment_id = ? AND delivery_id = ?
            ORDER BY timestamp DESC
            LIMIT 1";

        match self
            .client
            .query(query)
            .bind(deployment_id)
            .bind(delivery_id)
            .fetch_one::<WebhookLog>()
            .await
        {
            Ok(delivery) => {
                tracing::info!("Successfully fetched delivery details");
                Ok(delivery)
            }
            Err(e) => {
                tracing::error!("Failed to fetch delivery details: {:?}", e);
                Err(AppError::Internal(format!(
                    "Failed to fetch webhook delivery: {}",
                    e
                )))
            }
        }
    }

    pub async fn get_webhook_replay_source(
        &self,
        deployment_id: i64,
        delivery_id: i64,
    ) -> Result<WebhookReplaySourceRow, AppError> {
        let query = "SELECT
                app_slug,
                endpoint_id,
                event_name,
                status,
                max_attempts,
                payload,
                timestamp
            FROM webhook_logs_full
            WHERE deployment_id = ?
                AND delivery_id = ?
                AND payload IS NOT NULL
                AND endpoint_id > 0
            ORDER BY attempt_number DESC, timestamp DESC
            LIMIT 1";

        self.client
            .query(query)
            .bind(deployment_id)
            .bind(delivery_id)
            .fetch_one::<WebhookReplaySourceRow>()
            .await
            .map_err(|e| {
                AppError::NotFound(format!(
                    "Replay source not found for delivery {}: {}",
                    delivery_id, e
                ))
            })
    }

    pub async fn get_deliveries_for_replay(
        &self,
        deployment_id: i64,
        app_slug: String,
        start_date: DateTime<Utc>,
        end_date: Option<DateTime<Utc>>,
        status: Option<&str>,
        event_name: Option<&str>,
        endpoint_id: Option<i64>,
    ) -> Result<Vec<i64>, AppError> {
        let end_date = end_date.unwrap_or_else(Utc::now);

        let mut having_conditions = vec!["final_status != 'filtered'".to_string()];
        let mut having_binds: Vec<ClickHouseBind> = Vec::new();

        if let Some(status) = status {
            having_conditions.push("final_status = ?".to_string());
            having_binds.push(ClickHouseBind::String(status.to_string()));
        }
        if let Some(event_name) = event_name {
            having_conditions.push("final_event_name = ?".to_string());
            having_binds.push(ClickHouseBind::String(event_name.to_string()));
        }
        if let Some(endpoint_id) = endpoint_id {
            having_conditions.push("final_endpoint_id = ?".to_string());
            having_binds.push(ClickHouseBind::I64(endpoint_id));
        }

        let query = format!(
            "SELECT
                delivery_id,
                argMax(status, timestamp) as final_status,
                argMax(event_name, timestamp) as final_event_name,
                argMax(endpoint_id, timestamp) as final_endpoint_id
            FROM webhook_logs_light
            WHERE deployment_id = ?
                AND app_slug = ?
                AND timestamp >= ?
                AND timestamp <= ?
            GROUP BY delivery_id
            HAVING {}
            ORDER BY delivery_id DESC",
            having_conditions.join(" AND "),
        );

        let mut binds = vec![
            ClickHouseBind::I64(deployment_id),
            ClickHouseBind::String(app_slug),
            ClickHouseBind::DateTime(start_date),
            ClickHouseBind::DateTime(end_date),
        ];
        binds.extend(having_binds);

        tracing::info!("Fetching deliveries for replay with parameterized query");

        let mut cursor =
            bind_clickhouse_query(self.client.query(&query), &binds)
                .fetch::<(i64, String, String, i64)>()?;
        let mut delivery_ids = Vec::new();

        while let Some((delivery_id, _status, _event_name, _endpoint_id)) = cursor.next().await? {
            delivery_ids.push(delivery_id);
        }

        tracing::info!("Found {} deliveries to replay", delivery_ids.len());
        Ok(delivery_ids)
    }

    pub async fn get_deliveries_by_ids(
        &self,
        deployment_id: i64,
        delivery_ids: Vec<i64>,
    ) -> Result<Vec<i64>, AppError> {
        if delivery_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = vec!["?"; delivery_ids.len()].join(",");

        let query = format!(
            "SELECT
                delivery_id,
                argMax(status, timestamp) as final_status
            FROM webhook_logs_light
                WHERE deployment_id = ?
                AND delivery_id IN ({placeholders})
            GROUP BY delivery_id
            HAVING final_status != 'filtered'
            ORDER BY delivery_id DESC"
        );

        let mut binds = vec![ClickHouseBind::I64(deployment_id)];
        for delivery_id in &delivery_ids {
            binds.push(ClickHouseBind::I64(*delivery_id));
        }

        let mut cursor =
            bind_clickhouse_query(self.client.query(&query), &binds).fetch::<(i64, String)>()?;
        let mut valid_delivery_ids = Vec::new();

        while let Some((delivery_id, _status)) = cursor.next().await? {
            valid_delivery_ids.push(delivery_id);
        }

        Ok(valid_delivery_ids)
    }
}
