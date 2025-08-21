use crate::error::AppError;
use chrono::{DateTime, Utc};
use clickhouse::Row;
use serde::{Deserialize, Serialize};

use super::clickhouse::ClickHouseService;

use dto::clickhouse::webhook::*;

impl ClickHouseService {
    pub async fn init_webhook_tables(&self) -> Result<(), AppError> {
        let query = r#"
            CREATE TABLE IF NOT EXISTS webhook_events_local ON CLUSTER 'wacht_prod' (
                deployment_id Int64,
                app_name LowCardinality(String),
                event_name LowCardinality(String),
                event_id String,
                payload_size_bytes Int32,
                filter_context Nullable(String),
                timestamp DateTime64(6, 'UTC'),

                -- Indexes
                INDEX idx_deployment deployment_id TYPE minmax GRANULARITY 1,
                INDEX idx_app_name app_name TYPE bloom_filter(0.01) GRANULARITY 4,
                INDEX idx_event_name event_name TYPE bloom_filter(0.01) GRANULARITY 4
            )
            ENGINE = ReplicatedMergeTree('/clickhouse/tables/{shard}/webhook_events', '{replica}')
            PARTITION BY toYYYYMM(timestamp)
            ORDER BY (deployment_id, app_name, timestamp)
            TTL timestamp + INTERVAL 90 DAY
            SETTINGS
                storage_policy = 'tiered',
                index_granularity = 8192;
        "#;

        self.client.query(query).execute().await?;

        let query = r#"
            CREATE TABLE IF NOT EXISTS webhook_events ON CLUSTER 'wacht_prod' (
                deployment_id Int64,
                app_name String,
                event_name String,
                event_id String,
                payload_size_bytes Int32,
                filter_context Nullable(String),
                timestamp DateTime64(6, 'UTC')
            )
            ENGINE = Distributed(
                'wacht_prod',
                currentDatabase(),
                webhook_events_local,
                cityHash64(deployment_id, app_name)
            );
        "#;

        self.client.query(query).execute().await?;

        let query = r#"
            CREATE TABLE IF NOT EXISTS webhook_deliveries_local ON CLUSTER 'wacht_prod' (
                deployment_id Int64,
                delivery_id Int64,
                app_name LowCardinality(String),
                endpoint_id Int64,
                endpoint_url String,
                event_name LowCardinality(String),
                status LowCardinality(String),
                http_status_code Nullable(Int32),
                response_time_ms Nullable(Int32),
                attempt_number Int32,
                max_attempts Int32,
                error_message Nullable(String),
                filtered_reason Nullable(String),
                payload_s3_key String,
                response_body Nullable(String),
                response_headers Nullable(String),
                timestamp DateTime64(6, 'UTC'),

                -- Indexes
                INDEX idx_delivery_id delivery_id TYPE minmax GRANULARITY 4,
                INDEX idx_app_name app_name TYPE bloom_filter(0.01) GRANULARITY 4,
                INDEX idx_status status TYPE bloom_filter(0.01) GRANULARITY 4,
                INDEX idx_endpoint_id endpoint_id TYPE minmax GRANULARITY 4
            )
            ENGINE = ReplicatedMergeTree('/clickhouse/tables/{shard}/webhook_deliveries', '{replica}')
            PARTITION BY toYYYYMM(timestamp)
            ORDER BY (deployment_id, app_name, timestamp)
            TTL timestamp + INTERVAL 30 DAY
            SETTINGS
                storage_policy = 'tiered',
                index_granularity = 8192;
        "#;

        self.client.query(query).execute().await?;

        let query = r#"
            CREATE TABLE IF NOT EXISTS webhook_deliveries ON CLUSTER 'wacht_prod' (
                deployment_id Int64,
                delivery_id Int64,
                app_name String,
                endpoint_id Int64,
                endpoint_url String,
                event_name String,
                status String,
                http_status_code Nullable(Int32),
                response_time_ms Nullable(Int32),
                attempt_number Int32,
                max_attempts Int32,
                error_message Nullable(String),
                filtered_reason Nullable(String),
                payload_s3_key String,
                response_body Nullable(String),
                response_headers Nullable(String),
                timestamp DateTime64(6, 'UTC')
            )
            ENGINE = Distributed(
                'wacht_prod',
                currentDatabase(),
                webhook_deliveries_local,
                cityHash64(deployment_id, delivery_id)
            );
        "#;

        self.client.query(query).execute().await?;

        let query = r#"
            CREATE MATERIALIZED VIEW IF NOT EXISTS webhook_metrics_hourly ON CLUSTER 'wacht_prod'
            ENGINE = ReplicatedSummingMergeTree('/clickhouse/tables/{shard}/webhook_metrics_hourly', '{replica}')
            PARTITION BY toYYYYMM(time_bucket)
            ORDER BY (deployment_id, app_name, time_bucket)
            AS
            SELECT
                deployment_id,
                app_name,
                toStartOfHour(timestamp) as time_bucket,
                count() as total_deliveries,
                countIf(status = 'success') as successful_deliveries,
                countIf(status = 'failed') as failed_deliveries,
                countIf(status = 'filtered') as filtered_deliveries,
                avg(response_time_ms) as avg_response_time_ms,
                quantile(0.95)(response_time_ms) as p95_response_time_ms
            FROM webhook_deliveries_local
            GROUP BY deployment_id, app_name, time_bucket;
        "#;

        self.client.query(query).execute().await?;

        Ok(())
    }

    pub async fn insert_webhook_event(&self, event: &WebhookEvent) -> Result<(), AppError> {
        let mut insert = self.client.insert("webhook_events")?;
        insert.write(event).await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn insert_webhook_delivery(
        &self,
        delivery: &WebhookDelivery,
    ) -> Result<(), AppError> {
        let mut insert = self.client.insert("webhook_deliveries")?;
        insert.write(delivery).await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn batch_insert_webhook_events(
        &self,
        events: &[WebhookEvent],
    ) -> Result<(), AppError> {
        if events.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.insert("webhook_events")?;
        for event in events {
            insert.write(event).await?;
        }
        insert.end().await?;
        Ok(())
    }

    pub async fn batch_insert_webhook_deliveries(
        &self,
        deliveries: &[WebhookDelivery],
    ) -> Result<(), AppError> {
        if deliveries.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.insert("webhook_deliveries")?;
        for delivery in deliveries {
            insert.write(delivery).await?;
        }
        insert.end().await?;
        Ok(())
    }

    pub async fn get_webhook_delivery_stats(
        &self,
        deployment_id: i64,
        app_name: Option<String>,
        endpoint_id: Option<i64>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<WebhookDeliveryStatsRow, AppError> {
        let mut delivery_conditions = vec!["deployment_id = ?".to_string()];
        let mut delivery_bindings: Vec<String> = vec![deployment_id.to_string()];

        if let Some(ref app_name) = app_name {
            delivery_conditions.push("app_name = ?".to_string());
            delivery_bindings.push(app_name.clone());
        }

        if let Some(endpoint_id) = endpoint_id {
            delivery_conditions.push("endpoint_id = ?".to_string());
            delivery_bindings.push(endpoint_id.to_string());
        }

        delivery_conditions.push("timestamp >= ?".to_string());
        delivery_bindings.push(from.format("%Y-%m-%d %H:%M:%S%.6f").to_string());
        delivery_conditions.push("timestamp <= ?".to_string());
        delivery_bindings.push(to.format("%Y-%m-%d %H:%M:%S%.6f").to_string());

        let mut event_conditions = vec!["deployment_id = ?".to_string()];
        let mut event_bindings: Vec<String> = vec![deployment_id.to_string()];

        if let Some(ref app_name) = app_name {
            event_conditions.push("app_name = ?".to_string());
            event_bindings.push(app_name.clone());
        }

        event_conditions.push("timestamp >= ?".to_string());
        event_bindings.push(from.format("%Y-%m-%d %H:%M:%S%.6f").to_string());
        event_conditions.push("timestamp <= ?".to_string());
        event_bindings.push(to.format("%Y-%m-%d %H:%M:%S%.6f").to_string());

        let query = format!(
            r#"
                SELECT
                    CAST(0 AS Int64) as total_events,
                    CAST(count() AS Int64) as total_deliveries,
                    CAST(countIf(status = 'success') AS Int64) as successful_deliveries,
                    CAST(countIf(status IN ('failed', 'permanently_failed')) AS Int64) as failed_deliveries,
                    CAST(countIf(status = 'filtered') AS Int64) as filtered_deliveries,
                    CAST(avgOrNull(response_time_ms) AS Nullable(Float64)) as avg_response_time_ms,
                    CAST(quantileOrNull(0.5)(response_time_ms) AS Nullable(Float64)) as p50_response_time_ms,
                    CAST(quantileOrNull(0.95)(response_time_ms) AS Nullable(Float64)) as p95_response_time_ms,
                    CAST(quantileOrNull(0.99)(response_time_ms) AS Nullable(Float64)) as p99_response_time_ms
                FROM webhook_deliveries
                WHERE {}
            "#,
            delivery_conditions.join(" AND ")
        );

        let mut query_builder = self.client.query(&query);
        for binding in delivery_bindings {
            query_builder = query_builder.bind(binding);
        }

        let result = query_builder.fetch_one::<WebhookDeliveryStatsRow>().await?;

        Ok(result)
    }

    pub async fn get_webhook_event_distribution(
        &self,
        deployment_id: i64,
        app_name: Option<String>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<WebhookEventDistribution>, AppError> {
        let query = if app_name.is_some() {
            format!(
                r#"
                SELECT
                    event_name,
                    count() as count
                FROM webhook_events
                WHERE deployment_id = ? AND app_name = ? AND timestamp >= ? AND timestamp <= ?
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
                    count() as count
                FROM webhook_events
                WHERE deployment_id = ? AND timestamp >= ? AND timestamp <= ?
                GROUP BY event_name
                ORDER BY count DESC
                LIMIT {}
            "#,
                limit
            )
        };

        let rows = if let Some(ref app_name) = app_name {
            self.client
                .query(&query)
                .bind(deployment_id)
                .bind(app_name.clone())
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
                endpoint_url,
                count() as total_attempts,
                countIf(status = 'success') as successful_attempts,
                avg(response_time_ms) as avg_response_time,
                quantile(0.5)(response_time_ms) as p50_response_time,
                quantile(0.95)(response_time_ms) as p95_response_time,
                quantile(0.99)(response_time_ms) as p99_response_time,
                max(response_time_ms) as max_response_time,
                min(response_time_ms) as min_response_time
            FROM webhook_deliveries
            WHERE deployment_id = ? AND endpoint_id = ? AND timestamp >= ? AND timestamp <= ?
            GROUP BY endpoint_url
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
        app_name: Option<String>,
        endpoint_id: Option<i64>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<WebhookFailureReasonResponse>, AppError> {
        let mut where_conditions = vec!["deployment_id = ?".to_string()];
        let mut bindings: Vec<String> = vec![deployment_id.to_string()];

        if let Some(ref app_name) = app_name {
            where_conditions.push("app_name = ?".to_string());
            bindings.push(app_name.clone());
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
                        WHEN error_message LIKE '%timeout%' THEN 'Timeout'
                        ELSE coalesce(error_message, 'Unknown')
                    END as reason,
                    count() as count
                FROM webhook_deliveries
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
        app_name: String,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<WebhookEndpointStatsResponse>, AppError> {
        let query = r#"
            SELECT
                endpoint_id,
                endpoint_url,
                count() as total_attempts,
                countIf(status = 'success') as successful_attempts,
                countIf(status IN ('failed', 'permanently_failed')) as failed_attempts,
                avg(response_time_ms) as avg_response_time_ms
            FROM webhook_deliveries
            WHERE deployment_id = ? AND app_name = ? AND timestamp >= ? AND timestamp <= ?
            GROUP BY endpoint_id, endpoint_url
            ORDER BY total_attempts DESC
            LIMIT 20
        "#;

        let rows = self
            .client
            .query(query)
            .bind(deployment_id)
            .bind(app_name)
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
        app_name: Option<String>,
        endpoint_id: Option<i64>,
        interval: &models::webhook_analytics::TimeseriesInterval,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<WebhookTimeseriesResponse>, AppError> {
        let mut where_conditions = vec!["deployment_id = ?".to_string()];
        let mut bindings: Vec<String> = vec![deployment_id.to_string()];

        if let Some(ref app_name) = app_name {
            where_conditions.push("app_name = ?".to_string());
            bindings.push(app_name.clone());
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
                FROM webhook_deliveries
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

        // Also get event counts for the same time buckets
        // Build separate conditions for events table (doesn't have endpoint_id)
        let mut event_where_conditions = vec!["deployment_id = ?".to_string()];
        let mut event_bindings: Vec<String> = vec![deployment_id.to_string()];

        if let Some(ref app_name) = app_name {
            event_where_conditions.push("app_name = ?".to_string());
            event_bindings.push(app_name.clone());
        }

        event_where_conditions.push("timestamp >= ?".to_string());
        event_bindings.push(from.format("%Y-%m-%d %H:%M:%S%.6f").to_string());
        event_where_conditions.push("timestamp <= ?".to_string());
        event_bindings.push(to.format("%Y-%m-%d %H:%M:%S%.6f").to_string());

        let event_query = format!(
            r#"
                SELECT
                    toDateTime64({}(timestamp), 6) as bucket,
                    toInt64(count()) as total_events
                FROM webhook_events
                WHERE {}
                GROUP BY bucket
                ORDER BY bucket ASC
            "#,
            interval_fn,
            event_where_conditions.join(" AND ")
        );

        let mut event_query_builder = self.client.query(&event_query);
        for binding in &event_bindings {
            event_query_builder = event_query_builder.bind(binding.clone());
        }

        let event_rows = event_query_builder
            .fetch_all::<WebhookEventTimeseriesRow>()
            .await?;

        // Merge the results
        use std::collections::HashMap;
        let mut event_map: HashMap<DateTime<Utc>, i64> = HashMap::new();
        for row in event_rows {
            event_map.insert(row.bucket, row.total_events);
        }

        Ok(delivery_rows
            .into_iter()
            .map(|row| {
                let total_events = event_map.get(&row.bucket).copied().unwrap_or(0);
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
        app_name: Option<String>,
        status: Option<&str>,
        event_name: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<serde_json::Value>, AppError> {
        let mut query = format!(
            "SELECT
                delivery_id,
                app_name,
                endpoint_id,
                endpoint_url,
                event_name,
                status,
                http_status_code,
                response_time_ms,
                attempt_number,
                max_attempts,
                error_message,
                filtered_reason,
                response_headers,
                timestamp
            FROM webhook_deliveries
            WHERE deployment_id = {deployment_id}"
        );

        if let Some(ref app_name) = app_name {
            query.push_str(&format!(" AND app_name = '{app_name}'"));
        }

        if let Some(status) = status {
            query.push_str(&format!(" AND status = '{status}'"));
        }

        if let Some(event_name) = event_name {
            query.push_str(&format!(" AND event_name = '{event_name}'"));
        }

        query.push_str(&format!(
            " ORDER BY timestamp DESC LIMIT {limit} OFFSET {offset}"
        ));
        #[derive(Debug, Row, Deserialize)]
        struct DeliveryRow {
            delivery_id: i64,
            app_name: String,
            endpoint_id: i64,
            endpoint_url: String,
            event_name: String,
            status: String,
            http_status_code: Option<i32>,
            response_time_ms: Option<i32>,
            attempt_number: i32,
            max_attempts: i32,
            error_message: Option<String>,
            filtered_reason: Option<String>,
            response_headers: Option<String>,
            #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
            timestamp: DateTime<Utc>,
        }

        // Struct for JSON serialization with IDs as strings
        #[derive(Debug, Serialize)]
        struct DeliveryJson {
            #[serde(with = "models::utils::serde::i64_as_string")]
            delivery_id: i64,
            app_name: String,
            #[serde(with = "models::utils::serde::i64_as_string")]
            endpoint_id: i64,
            endpoint_url: String,
            event_name: String,
            status: String,
            http_status_code: Option<i32>,
            response_time_ms: Option<i32>,
            attempt_number: i32,
            max_attempts: i32,
            error_message: Option<String>,
            filtered_reason: Option<String>,
            response_headers: Option<String>,
            timestamp: DateTime<Utc>,
        }

        let rows = self.client.query(&query).fetch_all::<DeliveryRow>().await?;

        let results: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|row| DeliveryJson {
                delivery_id: row.delivery_id,
                app_name: row.app_name,
                endpoint_id: row.endpoint_id,
                endpoint_url: row.endpoint_url,
                event_name: row.event_name,
                status: row.status,
                http_status_code: row.http_status_code,
                response_time_ms: row.response_time_ms,
                attempt_number: row.attempt_number,
                max_attempts: row.max_attempts,
                error_message: row.error_message,
                filtered_reason: row.filtered_reason,
                response_headers: row.response_headers,
                timestamp: row.timestamp,
            })
            .filter_map(|row| serde_json::to_value(row).ok())
            .collect();

        eprintln!("Successfully fetched {} deliveries", results.len());
        Ok(results)
    }

    pub async fn get_webhook_delivery_details(
        &self,
        deployment_id: i64,
        delivery_id: i64,
    ) -> Result<serde_json::Value, AppError> {
        eprintln!(
            "Getting delivery details for deployment_id={}, delivery_id={}",
            deployment_id, delivery_id
        );
        let query = format!(
            "SELECT
                delivery_id,
                app_name,
                endpoint_id,
                endpoint_url,
                event_name,
                status,
                http_status_code,
                response_time_ms,
                attempt_number,
                max_attempts,
                error_message,
                filtered_reason,
                payload_s3_key,
                response_body,
                response_headers,
                timestamp
            FROM webhook_deliveries
            WHERE deployment_id = {deployment_id} AND delivery_id = {delivery_id}
                AND status != 'replayed'
            ORDER BY timestamp DESC
            LIMIT 1"
        );

        #[derive(Debug, Row, Deserialize)]
        struct DeliveryRow {
            delivery_id: i64,
            app_name: String,
            endpoint_id: i64,
            endpoint_url: String,
            event_name: String,
            status: String,
            http_status_code: Option<i32>,
            response_time_ms: Option<i32>,
            attempt_number: i32,
            max_attempts: i32,
            error_message: Option<String>,
            filtered_reason: Option<String>,
            payload_s3_key: String,
            response_body: Option<String>,
            response_headers: Option<String>,
            #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
            timestamp: DateTime<Utc>,
        }
        let mut cursor = self.client.query(&query).fetch::<DeliveryRow>()?;

        eprintln!("Fetching row from cursor...");
        if let Some(row) = cursor.next().await? {
            eprintln!(
                "Got row: delivery_id={}, status={}",
                row.delivery_id, row.status
            );

            let result = serde_json::json!({
                "delivery_id": row.delivery_id.to_string(),
                "app_name": row.app_name,
                "endpoint_id": row.endpoint_id.to_string(),
                "endpoint_url": row.endpoint_url,
                "event_name": row.event_name,
                "status": row.status,
                "http_status_code": row.http_status_code,
                "response_time_ms": row.response_time_ms,
                "attempt_number": row.attempt_number,
                "max_attempts": row.max_attempts,
                "error_message": row.error_message,
                "filtered_reason": row.filtered_reason,
                "payload_s3_key": row.payload_s3_key,
                "response_body": row.response_body,
                "response_headers": row.response_headers,
                "timestamp": row.timestamp,
            });

            eprintln!("Returning delivery details");
            Ok(result)
        } else {
            eprintln!("No row found for delivery_id={}", delivery_id);
            Err(AppError::NotFound("Delivery not found".to_string()))
        }
    }

    pub async fn cleanup_old_webhook_data(&self, days_to_keep: i32) -> Result<(), AppError> {
        let cutoff_date = Utc::now() - chrono::Duration::days(days_to_keep as i64);

        // ClickHouse TTL will handle most cleanup, but we can force cleanup if needed
        let query = format!(
            "ALTER TABLE webhook_events_local ON CLUSTER 'wacht_prod' DELETE WHERE timestamp < '{}'",
            cutoff_date.format("%Y-%m-%d %H:%M:%S")
        );

        self.client.query(&query).execute().await?;

        let query = format!(
            "ALTER TABLE webhook_deliveries_local ON CLUSTER 'wacht_prod' DELETE WHERE timestamp < '{}'",
            cutoff_date.format("%Y-%m-%d %H:%M:%S")
        );

        self.client.query(&query).execute().await?;

        Ok(())
    }
}
