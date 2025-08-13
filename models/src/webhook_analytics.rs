use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// Analytics result models
#[derive(Debug, Serialize)]
pub struct WebhookAnalyticsResult {
    pub total_events: i64,
    pub total_deliveries: i64,
    pub successful_deliveries: i64,
    pub failed_deliveries: i64,
    pub filtered_deliveries: i64,
    pub avg_response_time_ms: Option<f64>,
    pub p50_response_time_ms: Option<f64>,
    pub p95_response_time_ms: Option<f64>,
    pub p99_response_time_ms: Option<f64>,
    pub success_rate: f64,
    pub top_events: Vec<EventCount>,
    pub endpoint_performance: Vec<EndpointPerformance>,
    pub failure_reasons: Vec<FailureReason>,
}

#[derive(Debug, Serialize)]
pub struct EventCount {
    pub event_name: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct EndpointPerformance {
    pub endpoint_id: i64,
    pub endpoint_url: String,
    pub total_attempts: i64,
    pub successful_attempts: i64,
    pub failed_attempts: i64,
    pub avg_response_time_ms: Option<f64>,
    pub success_rate: f64,
}

#[derive(Debug, Serialize)]
pub struct FailureReason {
    pub reason: String,
    pub count: i64,
}

// Timeseries models
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum TimeseriesInterval {
    Hour,
    Day,
    Week,
    Month,
}

impl TimeseriesInterval {
    pub fn to_clickhouse_interval(&self) -> &'static str {
        match self {
            TimeseriesInterval::Hour => "toStartOfHour",
            TimeseriesInterval::Day => "toStartOfDay",
            TimeseriesInterval::Week => "toStartOfWeek",
            TimeseriesInterval::Month => "toStartOfMonth",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct WebhookTimeseriesResult {
    pub data: Vec<TimeseriesPoint>,
    pub interval: String,
}

#[derive(Debug, Serialize)]
pub struct TimeseriesPoint {
    pub timestamp: DateTime<Utc>,
    pub total_events: i64,
    pub total_deliveries: i64,
    pub successful_deliveries: i64,
    pub failed_deliveries: i64,
    pub filtered_deliveries: i64,
    pub avg_response_time_ms: Option<f64>,
    pub success_rate: f64,
}