use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

#[derive(Debug, Deserialize)]
pub struct WebhookEventTrigger {
    pub event_name: String,
    pub payload: Value,
    pub filter_context: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WebhookApp {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub signing_secret: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WebhookAppEvent {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub app_name: String,
    pub event_name: String,
    pub description: Option<String>,
    pub schema: Option<Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WebhookEndpoint {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub app_name: String,
    pub url: String,
    pub description: Option<String>,
    pub headers: Option<Value>,
    pub is_active: bool,
    pub signing_secret: Option<String>,
    pub max_retries: i32,
    pub timeout_seconds: i32,
    pub failure_count: i32,
    pub last_failure_at: Option<DateTime<Utc>>,
    pub auto_disabled: bool,
    pub auto_disabled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WebhookEndpointSubscription {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub endpoint_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub app_name: String,
    pub event_name: String,
    pub filter_rules: Option<Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ActiveWebhookDelivery {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub endpoint_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub app_name: String,
    pub event_name: String,
    pub payload_s3_key: String,
    pub payload_size_bytes: i32,
    pub webhook_id: String,
    pub webhook_timestamp: i64,
    pub signature: Option<String>,
    pub attempts: i32,
    pub max_attempts: i32,
    pub next_retry_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WebhookDeliveryStatus {
    Pending,
    Delivering,
    Delivered,
    Failed,
    Expired,
}

impl WebhookDeliveryStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Delivering => "delivering",
            Self::Delivered => "delivered",
            Self::Failed => "failed",
            Self::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookDeliveryAttempt {
    pub timestamp: DateTime<Utc>,
    pub status_code: Option<u16>,
    pub response_time_ms: Option<u32>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEventDefinition {
    pub name: String,
    pub description: String,
    pub schema: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookAppWithEvents {
    pub app: WebhookApp,
    pub events: Vec<WebhookAppEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEndpointWithSubscriptions {
    pub endpoint: WebhookEndpoint,
    pub subscribed_events: Vec<String>,
}

// Row type for pending deliveries from PostgreSQL
#[derive(FromRow)]
pub struct PendingDeliveryRow {
    pub delivery_id: i64,
    pub deployment_id: i64,
    pub app_name: String,
    pub endpoint_id: i64,
    pub endpoint_url: String,
    pub event_name: String,
    pub payload_s3_key: String,
    pub attempt_number: i32,
    pub max_attempts: i32,
    pub timestamp: DateTime<Utc>,
}
