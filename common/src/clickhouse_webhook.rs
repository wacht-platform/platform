use crate::error::AppError;
use chrono::{DateTime, Utc};
use clickhouse::Row;
use serde::{Deserialize, Serialize};

use super::clickhouse::ClickHouseService;

#[derive(Debug, Serialize, Deserialize, Row)]
pub struct WebhookEvent {
    pub deployment_id: i64,
    pub app_id: i64,
    pub app_name: String,
    pub event_name: String,
    pub event_id: String,  // Unique ID for deduplication
    pub payload_size_bytes: i32,
    pub payload_s3_key: Option<String>,
    pub filter_context: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Row)]
pub struct WebhookDelivery {
    pub deployment_id: i64,
    pub delivery_id: i64,
    pub app_id: i64,
    pub app_name: String,
    pub endpoint_id: i64,
    pub endpoint_url: String,
    pub event_name: String,
    pub status: String,  // 'pending', 'success', 'failed', 'filtered'
    pub http_status_code: Option<i32>,
    pub response_time_ms: Option<i32>,
    pub attempt_number: i32,
    pub error_message: Option<String>,
    pub filtered_reason: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Row)]
pub struct WebhookMetrics {
    pub deployment_id: i64,
    pub app_id: i64,
    pub app_name: String,
    pub time_bucket: DateTime<Utc>,
    pub total_events: i64,
    pub total_deliveries: i64,
    pub successful_deliveries: i64,
    pub failed_deliveries: i64,
    pub filtered_deliveries: i64,
    pub avg_response_time_ms: Option<f64>,
    pub p95_response_time_ms: Option<f64>,
    pub total_payload_bytes: i64,
}

#[derive(Debug, Serialize, Deserialize, Row)]
struct DeliveryStatsRow {
    total_events: i64,
    total_deliveries: i64,
    successful_deliveries: i64,
    failed_deliveries: i64,
    filtered_deliveries: i64,
    avg_response_time_ms: Option<f64>,
    p50_response_time_ms: Option<f64>,
    p95_response_time_ms: Option<f64>,
    p99_response_time_ms: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Row)]
struct EventStatsRow {
    event_name: String,
    count: i64,
}

impl ClickHouseService {
    pub async fn init_webhook_tables(&self) -> Result<(), AppError> {
        // Create webhook events table
        let query = r#"
            CREATE TABLE IF NOT EXISTS webhook_events_local ON CLUSTER 'wacht_prod' (
                deployment_id Int64,
                app_id Int64,
                app_name LowCardinality(String),
                event_name LowCardinality(String),
                event_id String,
                payload_size_bytes Int32,
                payload_s3_key Nullable(String),
                filter_context Nullable(String),
                timestamp DateTime64(3, 'UTC'),
                
                -- Indexes
                INDEX idx_app_id app_id TYPE minmax GRANULARITY 4,
                INDEX idx_event_name event_name TYPE bloom_filter(0.01) GRANULARITY 4,
                INDEX idx_deployment deployment_id TYPE minmax GRANULARITY 1
            )
            ENGINE = ReplicatedMergeTree('/clickhouse/tables/{shard}/webhook_events', '{replica}')
            PARTITION BY toYYYYMM(timestamp)
            ORDER BY (deployment_id, app_id, timestamp)
            TTL timestamp + INTERVAL 90 DAY
            SETTINGS
                storage_policy = 'tiered',
                index_granularity = 8192;
        "#;
        
        self.client.query(query).execute().await?;

        // Create distributed table for webhook events
        let query = r#"
            CREATE TABLE IF NOT EXISTS webhook_events ON CLUSTER 'wacht_prod' (
                deployment_id Int64,
                app_id Int64,
                app_name String,
                event_name String,
                event_id String,
                payload_size_bytes Int32,
                payload_s3_key Nullable(String),
                filter_context Nullable(String),
                timestamp DateTime64(3, 'UTC')
            )
            ENGINE = Distributed(
                'wacht_prod',
                currentDatabase(),
                webhook_events_local,
                cityHash64(deployment_id, app_id)
            );
        "#;
        
        self.client.query(query).execute().await?;

        // Create webhook deliveries table
        let query = r#"
            CREATE TABLE IF NOT EXISTS webhook_deliveries_local ON CLUSTER 'wacht_prod' (
                deployment_id Int64,
                delivery_id Int64,
                app_id Int64,
                app_name LowCardinality(String),
                endpoint_id Int64,
                endpoint_url String,
                event_name LowCardinality(String),
                status LowCardinality(String),
                http_status_code Nullable(Int32),
                response_time_ms Nullable(Int32),
                attempt_number Int32,
                error_message Nullable(String),
                filtered_reason Nullable(String),
                timestamp DateTime64(3, 'UTC'),
                
                -- Indexes
                INDEX idx_delivery_id delivery_id TYPE minmax GRANULARITY 4,
                INDEX idx_status status TYPE bloom_filter(0.01) GRANULARITY 4,
                INDEX idx_endpoint_id endpoint_id TYPE minmax GRANULARITY 4
            )
            ENGINE = ReplicatedMergeTree('/clickhouse/tables/{shard}/webhook_deliveries', '{replica}')
            PARTITION BY toYYYYMM(timestamp)
            ORDER BY (deployment_id, app_id, timestamp)
            TTL timestamp + INTERVAL 30 DAY
            SETTINGS
                storage_policy = 'tiered',
                index_granularity = 8192;
        "#;
        
        self.client.query(query).execute().await?;

        // Create distributed table for webhook deliveries
        let query = r#"
            CREATE TABLE IF NOT EXISTS webhook_deliveries ON CLUSTER 'wacht_prod' (
                deployment_id Int64,
                delivery_id Int64,
                app_id Int64,
                app_name String,
                endpoint_id Int64,
                endpoint_url String,
                event_name String,
                status String,
                http_status_code Nullable(Int32),
                response_time_ms Nullable(Int32),
                attempt_number Int32,
                error_message Nullable(String),
                filtered_reason Nullable(String),
                timestamp DateTime64(3, 'UTC')
            )
            ENGINE = Distributed(
                'wacht_prod',
                currentDatabase(),
                webhook_deliveries_local,
                cityHash64(deployment_id, delivery_id)
            );
        "#;
        
        self.client.query(query).execute().await?;

        // Create materialized view for hourly metrics
        let query = r#"
            CREATE MATERIALIZED VIEW IF NOT EXISTS webhook_metrics_hourly ON CLUSTER 'wacht_prod'
            ENGINE = ReplicatedSummingMergeTree('/clickhouse/tables/{shard}/webhook_metrics_hourly', '{replica}')
            PARTITION BY toYYYYMM(time_bucket)
            ORDER BY (deployment_id, app_id, time_bucket)
            AS
            SELECT
                deployment_id,
                app_id,
                app_name,
                toStartOfHour(timestamp) as time_bucket,
                count() as total_deliveries,
                countIf(status = 'success') as successful_deliveries,
                countIf(status = 'failed') as failed_deliveries,
                countIf(status = 'filtered') as filtered_deliveries,
                avg(response_time_ms) as avg_response_time_ms,
                quantile(0.95)(response_time_ms) as p95_response_time_ms
            FROM webhook_deliveries_local
            GROUP BY deployment_id, app_id, app_name, time_bucket;
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

    pub async fn insert_webhook_delivery(&self, delivery: &WebhookDelivery) -> Result<(), AppError> {
        let mut insert = self.client.insert("webhook_deliveries")?;
        insert.write(delivery).await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn batch_insert_webhook_events(&self, events: &[WebhookEvent]) -> Result<(), AppError> {
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

    pub async fn batch_insert_webhook_deliveries(&self, deliveries: &[WebhookDelivery]) -> Result<(), AppError> {
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
        app_id: Option<i64>,
        endpoint_id: Option<i64>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<DeliveryStats, AppError> {
        let mut where_conditions = vec!["deployment_id = ?".to_string()];
        let mut bindings: Vec<String> = vec![deployment_id.to_string()];
        
        if let Some(app_id) = app_id {
            where_conditions.push("app_id = ?".to_string());
            bindings.push(app_id.to_string());
        }
        
        if let Some(endpoint_id) = endpoint_id {
            where_conditions.push("endpoint_id = ?".to_string());
            bindings.push(endpoint_id.to_string());
        }
        
        where_conditions.push("timestamp >= ?".to_string());
        bindings.push(from.format("%Y-%m-%d %H:%M:%S").to_string());
        where_conditions.push("timestamp <= ?".to_string());
        bindings.push(to.format("%Y-%m-%d %H:%M:%S").to_string());

        let query = format!(
            r#"
                SELECT 
                    (SELECT count() FROM webhook_events WHERE {}) as total_events,
                    count() as total_deliveries,
                    countIf(status = 'success') as successful_deliveries,
                    countIf(status IN ('failed', 'permanently_failed')) as failed_deliveries,
                    countIf(status = 'filtered') as filtered_deliveries,
                    avg(response_time_ms) as avg_response_time_ms,
                    quantile(0.5)(response_time_ms) as p50_response_time_ms,
                    quantile(0.95)(response_time_ms) as p95_response_time_ms,
                    quantile(0.99)(response_time_ms) as p99_response_time_ms
                FROM webhook_deliveries
                WHERE {}
            "#,
            where_conditions[0..bindings.len()-2].join(" AND "),
            where_conditions.join(" AND ")
        );

        let mut query_builder = self.client.query(&query);
        for binding in bindings {
            query_builder = query_builder.bind(binding);
        }
        
        let result = query_builder.fetch_one::<DeliveryStatsRow>().await?;

        Ok(DeliveryStats {
            total_events: result.total_events,
            total_deliveries: result.total_deliveries,
            successful_deliveries: result.successful_deliveries,
            failed_deliveries: result.failed_deliveries,
            filtered_deliveries: result.filtered_deliveries,
            avg_response_time_ms: result.avg_response_time_ms,
            p50_response_time_ms: result.p50_response_time_ms,
            p95_response_time_ms: result.p95_response_time_ms,
            p99_response_time_ms: result.p99_response_time_ms,
        })
    }

    pub async fn get_webhook_event_distribution(
        &self,
        deployment_id: i64,
        app_id: Option<i64>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<EventDistribution>, AppError> {
        let query = if app_id.is_some() {
            format!(r#"
                SELECT 
                    event_name,
                    count() as count
                FROM webhook_events
                WHERE deployment_id = ? AND app_id = ? AND timestamp >= ? AND timestamp <= ?
                GROUP BY event_name
                ORDER BY count DESC
                LIMIT {}
            "#, limit)
        } else {
            format!(r#"
                SELECT 
                    event_name,
                    count() as count
                FROM webhook_events
                WHERE deployment_id = ? AND timestamp >= ? AND timestamp <= ?
                GROUP BY event_name
                ORDER BY count DESC
                LIMIT {}
            "#, limit)
        };

        let rows = if let Some(app_id) = app_id {
            self.client
                .query(&query)
                .bind(deployment_id)
                .bind(app_id)
                .bind(from.format("%Y-%m-%d %H:%M:%S").to_string())
                .bind(to.format("%Y-%m-%d %H:%M:%S").to_string())
                .fetch_all::<EventStatsRow>()
                .await?
        } else {
            self.client
                .query(&query)
                .bind(deployment_id)
                .bind(from.format("%Y-%m-%d %H:%M:%S").to_string())
                .bind(to.format("%Y-%m-%d %H:%M:%S").to_string())
                .fetch_all::<EventStatsRow>()
                .await?
        };

        Ok(rows.into_iter().map(|row| EventDistribution {
            event_name: row.event_name,
            count: row.count,
        }).collect())
    }

    pub async fn get_webhook_endpoint_performance(
        &self,
        deployment_id: i64,
        endpoint_id: i64,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<EndpointPerformance, AppError> {
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

        let result = self.client
            .query(query)
            .bind(deployment_id)
            .bind(endpoint_id)
            .bind(from.format("%Y-%m-%d %H:%M:%S").to_string())
            .bind(to.format("%Y-%m-%d %H:%M:%S").to_string())
            .fetch_one::<EndpointPerformanceRow>()
            .await?;

        Ok(EndpointPerformance::from(result))
    }

    pub async fn get_webhook_failure_reasons(
        &self,
        deployment_id: i64,
        app_id: Option<i64>,
        endpoint_id: Option<i64>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<FailureReason>, AppError> {
        let mut where_conditions = vec!["deployment_id = ?".to_string()];
        let mut bindings: Vec<String> = vec![deployment_id.to_string()];
        
        if let Some(app_id) = app_id {
            where_conditions.push("app_id = ?".to_string());
            bindings.push(app_id.to_string());
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
        
        let rows = query_builder.fetch_all::<FailureReasonRow>().await?;
        
        Ok(rows.into_iter().map(|row| FailureReason {
            reason: row.reason,
            count: row.count,
        }).collect())
    }

    pub async fn get_app_endpoints_performance(
        &self,
        deployment_id: i64,
        app_id: i64,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<EndpointPerformanceData>, AppError> {
        let query = r#"
            SELECT 
                endpoint_id,
                endpoint_url,
                count() as total_attempts,
                countIf(status = 'success') as successful_attempts,
                countIf(status IN ('failed', 'permanently_failed')) as failed_attempts,
                avg(response_time_ms) as avg_response_time_ms
            FROM webhook_deliveries
            WHERE deployment_id = ? AND app_id = ? AND timestamp >= ? AND timestamp <= ?
            GROUP BY endpoint_id, endpoint_url
            ORDER BY total_attempts DESC
            LIMIT 20
        "#;

        let rows = self.client
            .query(query)
            .bind(deployment_id)
            .bind(app_id)
            .bind(from.format("%Y-%m-%d %H:%M:%S").to_string())
            .bind(to.format("%Y-%m-%d %H:%M:%S").to_string())
            .fetch_all::<EndpointPerformanceDataRow>()
            .await?;

        Ok(rows.into_iter().map(|row| EndpointPerformanceData {
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
        }).collect())
    }

    pub async fn get_webhook_timeseries(
        &self,
        deployment_id: i64,
        app_id: Option<i64>,
        endpoint_id: Option<i64>,
        interval: &models::webhook_analytics::TimeseriesInterval,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<TimeseriesData>, AppError> {
        let mut where_conditions = vec!["deployment_id = ?".to_string()];
        let mut bindings: Vec<String> = vec![deployment_id.to_string()];
        
        if let Some(app_id) = app_id {
            where_conditions.push("app_id = ?".to_string());
            bindings.push(app_id.to_string());
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
                    {}(timestamp) as bucket,
                    count() as total_deliveries,
                    countIf(status = 'success') as successful_deliveries,
                    countIf(status IN ('failed', 'permanently_failed')) as failed_deliveries,
                    countIf(status = 'filtered') as filtered_deliveries,
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
        
        let delivery_rows = query_builder.fetch_all::<TimeseriesRow>().await?;
        
        // Also get event counts for the same time buckets  
        let event_where_len = if bindings.len() > 2 { bindings.len() - 2 } else { 1 };
        let event_query = format!(
            r#"
                SELECT 
                    {}(timestamp) as bucket,
                    count() as total_events
                FROM webhook_events
                WHERE {}
                GROUP BY bucket
                ORDER BY bucket ASC
            "#,
            interval_fn,
            where_conditions[0..event_where_len].join(" AND ")
        );
        
        let mut event_query_builder = self.client.query(&event_query);
        for binding in &bindings[0..event_where_len] {
            event_query_builder = event_query_builder.bind(binding.clone());
        }
        
        let event_rows = event_query_builder.fetch_all::<EventTimeseriesRow>().await?;
        
        // Merge the results
        use std::collections::HashMap;
        let mut event_map: HashMap<DateTime<Utc>, i64> = HashMap::new();
        for row in event_rows {
            event_map.insert(row.bucket, row.total_events);
        }
        
        Ok(delivery_rows.into_iter().map(|row| {
            let total_events = event_map.get(&row.bucket).copied().unwrap_or(0);
            let success_rate = if row.total_deliveries > 0 {
                (row.successful_deliveries as f64 / row.total_deliveries as f64) * 100.0
            } else {
                0.0
            };
            
            TimeseriesData {
                timestamp: row.bucket,
                total_events,
                total_deliveries: row.total_deliveries,
                successful_deliveries: row.successful_deliveries,
                failed_deliveries: row.failed_deliveries,
                filtered_deliveries: row.filtered_deliveries,
                avg_response_time_ms: row.avg_response_time_ms,
                success_rate,
            }
        }).collect())
    }

    pub async fn get_recent_webhook_deliveries(
        &self,
        deployment_id: i64,
        app_id: Option<i64>,
        status: Option<&str>,
        event_name: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>, AppError> {
        let mut query = format!(
            "SELECT 
                delivery_id,
                app_id,
                app_name,
                endpoint_id,
                endpoint_url,
                event_name,
                status,
                http_status_code,
                response_time_ms,
                attempt_number,
                error_message,
                filtered_reason,
                timestamp
            FROM webhook_deliveries
            WHERE deployment_id = {deployment_id}"
        );

        if let Some(app_id) = app_id {
            query.push_str(&format!(" AND app_id = {app_id}"));
        }

        if let Some(status) = status {
            query.push_str(&format!(" AND status = '{status}'"));
        }

        if let Some(event_name) = event_name {
            query.push_str(&format!(" AND event_name = '{event_name}'"));
        }

        query.push_str(&format!(" ORDER BY timestamp DESC LIMIT {limit}"));

        // Define a struct for the delivery row
        #[derive(Debug, Row, Serialize, Deserialize)]
        struct DeliveryRow {
            delivery_id: i64,
            app_id: i64,
            app_name: String,
            endpoint_id: i64,
            endpoint_url: String,
            event_name: String,
            status: String,
            http_status_code: Option<i32>,
            response_time_ms: Option<i32>,
            attempt_number: i32,
            error_message: Option<String>,
            filtered_reason: Option<String>,
            timestamp: DateTime<Utc>,
        }

        let mut cursor = self.client.query(&query).fetch::<DeliveryRow>()?;
        let mut results = Vec::new();
        
        while let Some(row) = cursor.next().await? {
            // Convert to JSON
            results.push(serde_json::to_value(row)?);
        }

        Ok(results)
    }

    pub async fn get_webhook_delivery_details(
        &self,
        deployment_id: i64,
        delivery_id: i64,
    ) -> Result<serde_json::Value, AppError> {
        let query = format!(
            "SELECT 
                delivery_id,
                app_id,
                app_name,
                endpoint_id,
                endpoint_url,
                event_name,
                status,
                http_status_code,
                response_time_ms,
                attempt_number,
                error_message,
                filtered_reason,
                timestamp
            FROM webhook_deliveries
            WHERE deployment_id = {deployment_id} AND delivery_id = {delivery_id}
            LIMIT 1"
        );

        // Define a struct for the delivery row
        #[derive(Debug, Row, Serialize, Deserialize)]
        struct DeliveryRow {
            delivery_id: i64,
            app_id: i64,
            app_name: String,
            endpoint_id: i64,
            endpoint_url: String,
            event_name: String,
            status: String,
            http_status_code: Option<i32>,
            response_time_ms: Option<i32>,
            attempt_number: i32,
            error_message: Option<String>,
            filtered_reason: Option<String>,
            timestamp: DateTime<Utc>,
        }

        let mut cursor = self.client.query(&query).fetch::<DeliveryRow>()?;
        
        if let Some(row) = cursor.next().await? {
            Ok(serde_json::to_value(row)?)
        } else {
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

#[derive(Debug, Serialize, Deserialize)]
pub struct DeliveryStats {
    pub total_events: i64,
    pub total_deliveries: i64,
    pub successful_deliveries: i64,
    pub failed_deliveries: i64,
    pub filtered_deliveries: i64,
    pub avg_response_time_ms: Option<f64>,
    pub p50_response_time_ms: Option<f64>,
    pub p95_response_time_ms: Option<f64>,
    pub p99_response_time_ms: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EventDistribution {
    pub event_name: String,
    pub count: i64,
}

#[derive(Debug, Serialize, Deserialize, Row)]
struct EndpointPerformanceRow {
    endpoint_url: String,
    total_attempts: i64,
    successful_attempts: i64,
    avg_response_time: Option<f64>,
    p50_response_time: Option<f64>,
    p95_response_time: Option<f64>,
    p99_response_time: Option<f64>,
    max_response_time: Option<i32>,
    min_response_time: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EndpointPerformance {
    pub endpoint_url: String,
    pub total_attempts: i64,
    pub successful_attempts: i64,
    pub success_rate: f64,
    pub avg_response_time_ms: Option<f64>,
    pub p50_response_time_ms: Option<f64>,
    pub p95_response_time_ms: Option<f64>,
    pub p99_response_time_ms: Option<f64>,
    pub max_response_time_ms: Option<i32>,
    pub min_response_time_ms: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Row)]
struct FailureReasonRow {
    reason: String,
    count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FailureReason {
    pub reason: String,
    pub count: i64,
}

#[derive(Debug, Serialize, Deserialize, Row)]
struct EndpointPerformanceDataRow {
    endpoint_id: i64,
    endpoint_url: String,
    total_attempts: i64,
    successful_attempts: i64,
    failed_attempts: i64,
    avg_response_time_ms: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EndpointPerformanceData {
    pub endpoint_id: i64,
    pub endpoint_url: String,
    pub total_attempts: i64,
    pub successful_attempts: i64,
    pub failed_attempts: i64,
    pub avg_response_time_ms: Option<f64>,
    pub success_rate: f64,
}

#[derive(Debug, Serialize, Deserialize, Row)]
struct TimeseriesRow {
    bucket: DateTime<Utc>,
    total_deliveries: i64,
    successful_deliveries: i64,
    failed_deliveries: i64,
    filtered_deliveries: i64,
    avg_response_time_ms: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Row)]
struct EventTimeseriesRow {
    bucket: DateTime<Utc>,
    total_events: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TimeseriesData {
    pub timestamp: DateTime<Utc>,
    pub total_events: i64,
    pub total_deliveries: i64,
    pub successful_deliveries: i64,
    pub failed_deliveries: i64,
    pub filtered_deliveries: i64,
    pub avg_response_time_ms: Option<f64>,
    pub success_rate: f64,
}

impl From<EndpointPerformanceRow> for EndpointPerformance {
    fn from(row: EndpointPerformanceRow) -> Self {
        Self {
            endpoint_url: row.endpoint_url,
            total_attempts: row.total_attempts,
            successful_attempts: row.successful_attempts,
            success_rate: if row.total_attempts > 0 {
                (row.successful_attempts as f64 / row.total_attempts as f64) * 100.0
            } else {
                0.0
            },
            avg_response_time_ms: row.avg_response_time,
            p50_response_time_ms: row.p50_response_time,
            p95_response_time_ms: row.p95_response_time,
            p99_response_time_ms: row.p99_response_time,
            max_response_time_ms: row.max_response_time,
            min_response_time_ms: row.min_response_time,
        }
    }
}