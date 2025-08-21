use chrono::{DateTime, Utc};
use clickhouse::Row;
use serde::{Deserialize, Serialize};

// Core event and delivery structs for ClickHouse storage
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct WebhookEvent {
    pub deployment_id: i64,
    pub app_name: String,
    pub event_name: String,
    pub event_id: String,
    pub payload_size_bytes: i32,
    pub filter_context: Option<String>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct WebhookDelivery {
    pub deployment_id: i64,
    pub delivery_id: i64,
    pub app_name: String,
    pub endpoint_id: i64,
    pub endpoint_url: String,
    pub event_name: String,
    pub status: String,
    pub http_status_code: Option<i32>,
    pub response_time_ms: Option<i32>,
    pub attempt_number: i32,
    pub max_attempts: i32,
    pub error_message: Option<String>,
    pub filtered_reason: Option<String>,
    pub payload_s3_key: String,
    pub response_body: Option<String>,
    pub response_headers: Option<String>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    pub timestamp: DateTime<Utc>,
}

// Aggregated metrics view
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct WebhookMetrics {
    pub deployment_id: i64,
    pub app_name: String,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
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

// Stats aggregation structs
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct WebhookDeliveryStatsRow {
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

// Event distribution analysis
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct WebhookEventDistributionRow {
    pub event_name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEventDistribution {
    pub event_name: String,
    pub count: i64,
}

// Endpoint performance metrics
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct WebhookEndpointPerformanceRow {
    pub endpoint_url: String,
    pub total_attempts: i64,
    pub successful_attempts: i64,
    pub avg_response_time: Option<f64>,
    pub p50_response_time: Option<f64>,
    pub p95_response_time: Option<f64>,
    pub p99_response_time: Option<f64>,
    pub max_response_time: Option<i32>,
    pub min_response_time: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEndpointPerformanceResponse {
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

impl From<WebhookEndpointPerformanceRow> for WebhookEndpointPerformanceResponse {
    fn from(row: WebhookEndpointPerformanceRow) -> Self {
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

// Multiple endpoint performance data
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct WebhookEndpointStatsRow {
    pub endpoint_id: i64,
    pub endpoint_url: String,
    pub total_attempts: i64,
    pub successful_attempts: i64,
    pub failed_attempts: i64,
    pub avg_response_time_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEndpointStatsResponse {
    pub endpoint_id: i64,
    pub endpoint_url: String,
    pub total_attempts: i64,
    pub successful_attempts: i64,
    pub failed_attempts: i64,
    pub avg_response_time_ms: Option<f64>,
    pub success_rate: f64,
}

impl From<WebhookEndpointStatsRow> for WebhookEndpointStatsResponse {
    fn from(row: WebhookEndpointStatsRow) -> Self {
        Self {
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
        }
    }
}

// Failure analysis
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct WebhookFailureReasonRow {
    pub reason: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookFailureReasonResponse {
    pub reason: String,
    pub count: i64,
}

impl From<WebhookFailureReasonRow> for WebhookFailureReasonResponse {
    fn from(row: WebhookFailureReasonRow) -> Self {
        Self {
            reason: row.reason,
            count: row.count,
        }
    }
}

// Timeseries data
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct WebhookDeliveryTimeseriesRow {
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    pub bucket: DateTime<Utc>,
    pub total_deliveries: i64,
    pub successful_deliveries: i64,
    pub failed_deliveries: i64,
    pub filtered_deliveries: i64,
    pub avg_response_time_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct WebhookEventTimeseriesRow {
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    pub bucket: DateTime<Utc>,
    pub total_events: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookTimeseriesResponse {
    pub timestamp: DateTime<Utc>,
    pub total_events: i64,
    pub total_deliveries: i64,
    pub successful_deliveries: i64,
    pub failed_deliveries: i64,
    pub filtered_deliveries: i64,
    pub avg_response_time_ms: Option<f64>,
    pub success_rate: f64,
}

// Delivery list/detail rows - reusable for multiple queries
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct WebhookDeliveryListRow {
    pub delivery_id: i64,
    pub app_id: i64,
    pub app_name: String,
    pub endpoint_id: i64,
    pub endpoint_url: String,
    pub event_name: String,
    pub status: String,
    pub http_status_code: Option<i32>,
    pub response_time_ms: Option<i32>,
    pub attempt_number: i32,
    pub max_attempts: i32,
    pub error_message: Option<String>,
    pub filtered_reason: Option<String>,
    pub response_headers: Option<String>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    pub timestamp: DateTime<Utc>,
}

// Extended delivery row with payload data for detail views
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct WebhookDeliveryDetailRow {
    pub delivery_id: i64,
    pub app_id: i64,
    pub app_name: String,
    pub endpoint_id: i64,
    pub endpoint_url: String,
    pub event_name: String,
    pub status: String,
    pub http_status_code: Option<i32>,
    pub response_time_ms: Option<i32>,
    pub attempt_number: i32,
    pub max_attempts: i32,
    pub error_message: Option<String>,
    pub filtered_reason: Option<String>,
    pub payload_s3_key: String,
    pub response_body: Option<String>,
    pub response_headers: Option<String>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    pub timestamp: DateTime<Utc>,
}

// JSON serialization structs with string IDs for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookDeliveryListResponse {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub delivery_id: i64,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub app_id: i64,
    pub app_name: String,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub endpoint_id: i64,
    pub endpoint_url: String,
    pub event_name: String,
    pub status: String,
    pub http_status_code: Option<i32>,
    pub response_time_ms: Option<i32>,
    pub attempt_number: i32,
    pub max_attempts: i32,
    pub error_message: Option<String>,
    pub filtered_reason: Option<String>,
    pub response_headers: Option<String>,
    pub timestamp: DateTime<Utc>,
}

impl From<WebhookDeliveryListRow> for WebhookDeliveryListResponse {
    fn from(row: WebhookDeliveryListRow) -> Self {
        Self {
            delivery_id: row.delivery_id,
            app_id: row.app_id,
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
        }
    }
}

impl From<WebhookDeliveryDetailRow> for WebhookDeliveryListResponse {
    fn from(row: WebhookDeliveryDetailRow) -> Self {
        Self {
            delivery_id: row.delivery_id,
            app_id: row.app_id,
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
        }
    }
}
