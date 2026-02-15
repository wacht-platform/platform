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
    pub app_slug: String,
    pub name: String,
    pub description: Option<String>,
    pub signing_secret: String,
    pub failure_notification_emails: serde_json::Value,
    pub event_catalog_slug: Option<String>, // Added for shared event catalogs
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WebhookEventCatalog {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub events: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub duration_ms: i64,  // Window duration in milliseconds
    pub max_requests: i32, // Max requests in that window
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WebhookEndpoint {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub app_slug: String,
    pub url: String,
    pub description: Option<String>,
    pub headers: Option<Value>,
    pub is_active: bool,
    pub max_retries: i32,
    pub timeout_seconds: i32,
    pub failure_count: i32,
    pub last_failure_at: Option<DateTime<Utc>>,
    pub auto_disabled: bool,
    pub auto_disabled_at: Option<DateTime<Utc>>,
    pub rate_limit_config: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WebhookEndpoint {
    /// Parse rate limit config from JSONB
    pub fn get_rate_limit(&self) -> Option<RateLimitConfig> {
        self.rate_limit_config
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WebhookEndpointSubscription {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub endpoint_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub app_slug: String,
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
    pub app_slug: String,
    pub event_name: String,
    pub payload: Option<Value>,
    pub filter_rules: Option<Value>,
    pub payload_size_bytes: i32,
    pub webhook_id: String,
    pub webhook_timestamp: i64,
    pub signature: Option<String>,
    pub attempts: i32,
    pub max_attempts: i32,
    pub next_retry_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    #[sqlx(skip)]
    pub url: Option<String>,
    #[sqlx(skip)]
    pub timeout_seconds: Option<i32>,
    #[sqlx(skip)]
    pub headers: Option<Value>,
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
    pub group: Option<String>,
    pub schema: Option<Value>,
    pub example_payload: Option<Value>,
    #[serde(default)]
    pub is_archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEndpointWithSubscriptions {
    pub endpoint: WebhookEndpoint,
    pub subscribed_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookCatalogWithEvents {
    pub catalog: WebhookEventCatalog,
    pub events: Vec<WebhookEventDefinition>,
}

// Row type for pending deliveries from PostgreSQL
#[derive(FromRow)]
pub struct PendingDeliveryRow {
    pub delivery_id: i64,
    pub deployment_id: i64,
    pub app_slug: String,
    pub endpoint_id: i64,
    pub endpoint_url: String,
    pub event_name: String,
    pub payload: Option<Value>,
    pub attempt_number: i32,
    pub max_attempts: i32,
    pub timestamp: DateTime<Utc>,
}
