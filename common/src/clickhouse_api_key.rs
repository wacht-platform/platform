use crate::error::AppError;
use chrono::{DateTime, Utc};
use clickhouse::Row;
use serde::{Deserialize, Serialize};

use super::clickhouse::ClickHouseService;

#[derive(Debug, Serialize, Deserialize, Row)]
pub struct ApiKeyUsageEvent {
    pub deployment_id: i64,
    pub app_id: i64,
    pub app_name: String,
    pub key_id: i64,
    pub key_prefix: String,
    pub key_suffix: String,
    pub endpoint: String,            // API endpoint accessed
    pub method: String,               // HTTP method (GET, POST, etc.)
    pub status: String,               // 'success', 'failed', 'rate_limited', 'expired'
    pub http_status_code: i32,
    pub response_time_ms: i32,
    pub request_size_bytes: i32,
    pub response_size_bytes: i32,
    pub ip_address: String,
    pub user_agent: Option<String>,
    pub error_message: Option<String>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Row)]
pub struct ApiKeyMetrics {
    pub deployment_id: i64,
    pub app_id: i64,
    pub app_name: String,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    pub time_bucket: DateTime<Utc>,
    pub total_requests: i64,
    pub successful_requests: i64,
    pub failed_requests: i64,
    pub rate_limited_requests: i64,
    pub unique_keys_used: i64,
    pub unique_ips: i64,
    pub avg_response_time_ms: Option<f64>,
    pub p95_response_time_ms: Option<f64>,
    pub p99_response_time_ms: Option<f64>,
    pub total_request_bytes: i64,
    pub total_response_bytes: i64,
}

#[derive(Debug, Serialize, Deserialize, Row)]
pub struct ApiKeyEndpointStats {
    pub endpoint: String,
    pub method: String,
    pub total_calls: i64,
    pub success_rate: f64,
    pub avg_response_time_ms: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Row)]
pub struct ApiKeyUsageByKey {
    pub key_id: i64,
    pub key_prefix: String,
    pub key_suffix: String,
    pub total_requests: i64,
    pub successful_requests: i64,
    pub failed_requests: i64,
    pub last_used_at: DateTime<Utc>,
}

impl ClickHouseService {
    pub async fn init_api_key_tables(&self) -> Result<(), AppError> {
        // Create API key usage events table (local)
        let query = r#"
            CREATE TABLE IF NOT EXISTS api_key_usage_local ON CLUSTER 'wacht_prod' (
                deployment_id Int64,
                app_id Int64,
                app_name LowCardinality(String),
                key_id Int64,
                key_prefix LowCardinality(String),
                key_suffix String,
                endpoint LowCardinality(String),
                method LowCardinality(String),
                status LowCardinality(String),
                http_status_code Int32,
                response_time_ms Int32,
                request_size_bytes Int32,
                response_size_bytes Int32,
                ip_address String,
                user_agent Nullable(String),
                error_message Nullable(String),
                timestamp DateTime64(6, 'UTC'),

                -- Indexes for efficient querying
                INDEX idx_key_id key_id TYPE minmax GRANULARITY 4,
                INDEX idx_status status TYPE bloom_filter(0.01) GRANULARITY 4,
                INDEX idx_endpoint endpoint TYPE bloom_filter(0.01) GRANULARITY 4,
                INDEX idx_deployment deployment_id TYPE minmax GRANULARITY 1,
                INDEX idx_ip_address ip_address TYPE bloom_filter(0.01) GRANULARITY 8
            )
            ENGINE = ReplicatedMergeTree('/clickhouse/tables/{shard}/api_key_usage', '{replica}')
            PARTITION BY toYYYYMM(timestamp)
            ORDER BY (deployment_id, app_id, timestamp, key_id)
            TTL timestamp + INTERVAL 15 DAY TO VOLUME 'cold',
                timestamp + INTERVAL 90 DAY DELETE
            SETTINGS
                storage_policy = 'tiered',
                index_granularity = 8192;
        "#;

        self.client.query(query).execute().await?;

        // Create distributed table for API key usage
        let query = r#"
            CREATE TABLE IF NOT EXISTS api_key_usage ON CLUSTER 'wacht_prod' (
                deployment_id Int64,
                app_id Int64,
                app_name String,
                key_id Int64,
                key_prefix String,
                key_suffix String,
                endpoint String,
                method String,
                status String,
                http_status_code Int32,
                response_time_ms Int32,
                request_size_bytes Int32,
                response_size_bytes Int32,
                ip_address String,
                user_agent Nullable(String),
                error_message Nullable(String),
                timestamp DateTime64(6, 'UTC')
            )
            ENGINE = Distributed(
                'wacht_prod',
                currentDatabase(),
                api_key_usage_local,
                cityHash64(deployment_id, key_id)
            );
        "#;

        self.client.query(query).execute().await?;

        // Create materialized view for hourly metrics
        let query = r#"
            CREATE MATERIALIZED VIEW IF NOT EXISTS api_key_metrics_hourly ON CLUSTER 'wacht_prod'
            ENGINE = ReplicatedSummingMergeTree('/clickhouse/tables/{shard}/api_key_metrics_hourly', '{replica}')
            PARTITION BY toYYYYMM(time_bucket)
            ORDER BY (deployment_id, app_id, time_bucket)
            TTL time_bucket + INTERVAL 15 DAY TO VOLUME 'cold',
                time_bucket + INTERVAL 180 DAY DELETE
            SETTINGS storage_policy = 'tiered'
            AS
            SELECT
                deployment_id,
                app_id,
                any(app_name) AS app_name,
                toStartOfHour(timestamp) AS time_bucket,
                count() AS total_requests,
                countIf(status = 'success') AS successful_requests,
                countIf(status = 'failed') AS failed_requests,
                countIf(status = 'rate_limited') AS rate_limited_requests,
                uniqExact(key_id) AS unique_keys_used,
                uniqExact(ip_address) AS unique_ips,
                avgIf(response_time_ms, status = 'success') AS avg_response_time_ms,
                quantileIf(0.95)(response_time_ms, status = 'success') AS p95_response_time_ms,
                quantileIf(0.99)(response_time_ms, status = 'success') AS p99_response_time_ms,
                sum(request_size_bytes) AS total_request_bytes,
                sum(response_size_bytes) AS total_response_bytes
            FROM api_key_usage_local
            GROUP BY deployment_id, app_id, time_bucket;
        "#;

        self.client.query(query).execute().await?;

        // Create materialized view for daily metrics
        let query = r#"
            CREATE MATERIALIZED VIEW IF NOT EXISTS api_key_metrics_daily ON CLUSTER 'wacht_prod'
            ENGINE = ReplicatedSummingMergeTree('/clickhouse/tables/{shard}/api_key_metrics_daily', '{replica}')
            PARTITION BY toYYYYMM(time_bucket)
            ORDER BY (deployment_id, app_id, time_bucket)
            TTL time_bucket + INTERVAL 15 DAY TO VOLUME 'cold',
                time_bucket + INTERVAL 365 DAY DELETE
            SETTINGS storage_policy = 'tiered'
            AS
            SELECT
                deployment_id,
                app_id,
                any(app_name) AS app_name,
                toStartOfDay(timestamp) AS time_bucket,
                count() AS total_requests,
                countIf(status = 'success') AS successful_requests,
                countIf(status = 'failed') AS failed_requests,
                countIf(status = 'rate_limited') AS rate_limited_requests,
                uniqExact(key_id) AS unique_keys_used,
                uniqExact(ip_address) AS unique_ips,
                avgIf(response_time_ms, status = 'success') AS avg_response_time_ms,
                quantileIf(0.95)(response_time_ms, status = 'success') AS p95_response_time_ms,
                quantileIf(0.99)(response_time_ms, status = 'success') AS p99_response_time_ms,
                sum(request_size_bytes) AS total_request_bytes,
                sum(response_size_bytes) AS total_response_bytes
            FROM api_key_usage_local
            GROUP BY deployment_id, app_id, time_bucket;
        "#;

        self.client.query(query).execute().await?;

        // Create materialized view for endpoint statistics
        let query = r#"
            CREATE MATERIALIZED VIEW IF NOT EXISTS api_key_endpoint_stats ON CLUSTER 'wacht_prod'
            ENGINE = ReplicatedReplacingMergeTree('/clickhouse/tables/{shard}/api_key_endpoint_stats', '{replica}')
            PARTITION BY (deployment_id, toYYYYMM(last_update))
            ORDER BY (deployment_id, app_id, endpoint, method)
            AS
            SELECT
                deployment_id,
                app_id,
                endpoint,
                method,
                count() AS total_calls,
                countIf(status = 'success') / count() AS success_rate,
                avgIf(response_time_ms, status = 'success') AS avg_response_time_ms,
                max(timestamp) AS last_update
            FROM api_key_usage_local
            GROUP BY deployment_id, app_id, endpoint, method;
        "#;

        self.client.query(query).execute().await?;

        // Create materialized view for per-key usage statistics
        let query = r#"
            CREATE MATERIALIZED VIEW IF NOT EXISTS api_key_usage_by_key ON CLUSTER 'wacht_prod'
            ENGINE = ReplicatedReplacingMergeTree('/clickhouse/tables/{shard}/api_key_usage_by_key', '{replica}')
            PARTITION BY (deployment_id, toYYYYMM(last_used_at))
            ORDER BY (deployment_id, app_id, key_id)
            AS
            SELECT
                deployment_id,
                app_id,
                key_id,
                any(key_prefix) AS key_prefix,
                any(key_suffix) AS key_suffix,
                count() AS total_requests,
                countIf(status = 'success') AS successful_requests,
                countIf(status = 'failed') AS failed_requests,
                max(timestamp) AS last_used_at
            FROM api_key_usage_local
            GROUP BY deployment_id, app_id, key_id;
        "#;

        self.client.query(query).execute().await?;

        Ok(())
    }

    // Insert API key usage event
    pub async fn insert_api_key_usage(&self, event: ApiKeyUsageEvent) -> Result<(), AppError> {
        let mut insert = self.client.insert("api_key_usage")?;
        insert.write(&event).await?;
        insert.end().await?;
        Ok(())
    }

    // Batch insert API key usage events
    pub async fn insert_api_key_usage_batch(&self, events: Vec<ApiKeyUsageEvent>) -> Result<(), AppError> {
        if events.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.insert("api_key_usage")?;
        for event in events {
            insert.write(&event).await?;
        }
        insert.end().await?;
        Ok(())
    }

    // Get API key statistics for a deployment
    pub async fn get_api_key_stats(
        &self,
        deployment_id: i64,
        app_id: Option<i64>,
        start_date: Option<DateTime<Utc>>,
        end_date: Option<DateTime<Utc>>,
    ) -> Result<ApiKeyStats, AppError> {
        let mut query = String::from(
            "SELECT
                COUNT() as total_requests,
                countIf(status = 'success') as successful_requests,
                countIf(status = 'failed') as failed_requests,
                countIf(status = 'rate_limited') as rate_limited_requests,
                uniqExact(key_id) as unique_keys_used,
                avgIf(response_time_ms, status = 'success') as avg_response_time_ms,
                quantileIf(0.95)(response_time_ms, status = 'success') as p95_response_time_ms,
                quantileIf(0.99)(response_time_ms, status = 'success') as p99_response_time_ms
            FROM api_key_usage
            WHERE deployment_id = ?"
        );

        let mut params = vec![deployment_id.to_string()];

        if let Some(app_id) = app_id {
            query.push_str(" AND app_id = ?");
            params.push(app_id.to_string());
        }

        if let Some(start) = start_date {
            query.push_str(" AND timestamp >= ?");
            params.push(start.to_rfc3339());
        }

        if let Some(end) = end_date {
            query.push_str(" AND timestamp <= ?");
            params.push(end.to_rfc3339());
        }

        let result = self.client
            .query(&query)
            .bind(deployment_id)
            .fetch_one::<ApiKeyStatsRow>()
            .await?;

        Ok(ApiKeyStats {
            total_requests: result.total_requests,
            successful_requests: result.successful_requests,
            failed_requests: result.failed_requests,
            rate_limited_requests: result.rate_limited_requests,
            unique_keys_used: result.unique_keys_used,
            avg_response_time_ms: result.avg_response_time_ms,
            p95_response_time_ms: result.p95_response_time_ms,
            p99_response_time_ms: result.p99_response_time_ms,
            success_rate: if result.total_requests > 0 {
                (result.successful_requests as f64 / result.total_requests as f64) * 100.0
            } else {
                0.0
            },
        })
    }

    // Get top endpoints by API key usage
    pub async fn get_top_endpoints(
        &self,
        deployment_id: i64,
        app_id: Option<i64>,
        limit: usize,
    ) -> Result<Vec<ApiKeyEndpointStats>, AppError> {
        let mut query = String::from(
            "SELECT
                endpoint,
                method,
                count() as total_calls,
                countIf(status = 'success') / count() as success_rate,
                avgIf(response_time_ms, status = 'success') as avg_response_time_ms
            FROM api_key_usage
            WHERE deployment_id = ?"
        );

        if let Some(_app_id) = app_id {
            query.push_str(" AND app_id = ?");
        }

        query.push_str(" GROUP BY endpoint, method ORDER BY total_calls DESC LIMIT ?");

        let mut cursor = self.client
            .query(&query)
            .bind(deployment_id);

        if let Some(app_id) = app_id {
            cursor = cursor.bind(app_id);
        }

        cursor = cursor.bind(limit as i32);

        let result = cursor.fetch_all::<ApiKeyEndpointStats>().await?;
        Ok(result)
    }

    // Get usage by individual API keys
    pub async fn get_usage_by_keys(
        &self,
        deployment_id: i64,
        app_id: i64,
    ) -> Result<Vec<ApiKeyUsageByKey>, AppError> {
        let query = "
            SELECT
                key_id,
                any(key_prefix) as key_prefix,
                any(key_suffix) as key_suffix,
                count() as total_requests,
                countIf(status = 'success') as successful_requests,
                countIf(status = 'failed') as failed_requests,
                max(timestamp) as last_used_at
            FROM api_key_usage
            WHERE deployment_id = ? AND app_id = ?
                AND timestamp >= now() - INTERVAL 30 DAY
            GROUP BY key_id
            ORDER BY total_requests DESC
        ";

        let result = self.client
            .query(query)
            .bind(deployment_id)
            .bind(app_id)
            .fetch_all::<ApiKeyUsageByKey>()
            .await?;

        Ok(result)
    }

    // Get time series data for API key usage
    pub async fn get_api_key_timeseries(
        &self,
        deployment_id: i64,
        app_id: Option<i64>,
        interval: &str, // 'hour' or 'day'
        start_date: Option<DateTime<Utc>>,
        end_date: Option<DateTime<Utc>>,
    ) -> Result<Vec<ApiKeyMetrics>, AppError> {
        let table = match interval {
            "hour" => "api_key_metrics_hourly",
            "day" => "api_key_metrics_daily",
            _ => "api_key_metrics_hourly",
        };

        let mut query = format!(
            "SELECT * FROM {} WHERE deployment_id = ?",
            table
        );

        let mut params = vec![deployment_id.to_string()];

        if let Some(app_id) = app_id {
            query.push_str(" AND app_id = ?");
            params.push(app_id.to_string());
        }

        if let Some(start) = start_date {
            query.push_str(" AND time_bucket >= ?");
            params.push(start.to_rfc3339());
        }

        if let Some(end) = end_date {
            query.push_str(" AND time_bucket <= ?");
            params.push(end.to_rfc3339());
        }

        query.push_str(" ORDER BY time_bucket ASC");

        let mut cursor = self.client.query(&query).bind(deployment_id);

        if let Some(app_id) = app_id {
            cursor = cursor.bind(app_id);
        }

        if let Some(start) = start_date {
            cursor = cursor.bind(start);
        }

        if let Some(end) = end_date {
            cursor = cursor.bind(end);
        }

        let result = cursor.fetch_all::<ApiKeyMetrics>().await?;
        Ok(result)
    }
}

#[derive(Debug, Serialize, Deserialize, Row)]
struct ApiKeyStatsRow {
    total_requests: i64,
    successful_requests: i64,
    failed_requests: i64,
    rate_limited_requests: i64,
    unique_keys_used: i64,
    avg_response_time_ms: Option<f64>,
    p95_response_time_ms: Option<f64>,
    p99_response_time_ms: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyStats {
    pub total_requests: i64,
    pub successful_requests: i64,
    pub failed_requests: i64,
    pub rate_limited_requests: i64,
    pub unique_keys_used: i64,
    pub avg_response_time_ms: Option<f64>,
    pub p95_response_time_ms: Option<f64>,
    pub p99_response_time_ms: Option<f64>,
    pub success_rate: f64,
}