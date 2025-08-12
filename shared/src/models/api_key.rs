use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKeyApp {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub rate_limit_per_minute: Option<i32>,
    pub rate_limit_per_hour: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKey {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub app_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub name: String,
    pub key_prefix: String,
    pub key_suffix: String,
    #[serde(skip_serializing)]
    pub key_hash: String,
    #[sqlx(json)]
    pub permissions: Vec<String>,
    #[sqlx(json)]
    pub metadata: Value,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyWithSecret {
    #[serde(flatten)]
    pub key: ApiKey,
    pub secret: String, // Full key, only shown once
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKeyScope {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub api_key_id: i64,
    pub resource_type: String,
    pub resource_id: Option<String>,
    #[sqlx(json)]
    pub actions: Vec<String>,
    pub created_at: DateTime<Utc>,
}