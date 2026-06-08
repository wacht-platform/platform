use chrono::{DateTime, Utc};
use clickhouse::Row;
use serde::{Deserialize, Serialize};

/// One LLM call's token usage, stored raw in ClickHouse. Per-minute (or any)
/// rollups are a query concern (`GROUP BY toStartOfMinute(timestamp)`), not a
/// storage one. Separate from billing.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct ModelTokenUsageEvent {
    pub deployment_id: i64,
    pub model: String,
    pub thread_id: i64,
    pub actor_id: i64,
    pub is_byok: u8,
    pub input_tokens: i64,
    pub cached_tokens: i64,
    pub output_tokens: i64,
    pub thoughts_tokens: i64,
    pub total_tokens: i64,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    pub timestamp: DateTime<Utc>,
}

/// One time bucket of aggregated usage for a deployment.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct TokenUsageBucket {
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub bucket: DateTime<Utc>,
    pub input_tokens: i64,
    pub cached_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub request_count: u64,
}

/// One time bucket of API gateway (api-key verification) usage.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct GatewayUsageBucket {
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub bucket: DateTime<Utc>,
    pub total_requests: i64,
    pub allowed_requests: i64,
    pub blocked_requests: i64,
}

/// One time bucket of webhook delivery outcomes (from webhook_logs_light).
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct WebhookUsageBucket {
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub bucket: DateTime<Utc>,
    pub total_deliveries: i64,
    pub successful_deliveries: i64,
    pub failed_deliveries: i64,
    pub filtered_deliveries: i64,
}
