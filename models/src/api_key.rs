use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, Type};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "snake_case")]
pub enum RateLimitMode {
    #[sqlx(rename = "per_key")]
    PerKey, // Rate limit is applied per API key
    #[sqlx(rename = "per_ip")]
    PerIp, // Rate limit is applied per IP address
    #[sqlx(rename = "per_key_and_ip")]
    PerKeyAndIp, // Rate limit is applied per combination of key and IP
}

impl RateLimitMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PerKey => "per_key",
            Self::PerIp => "per_ip",
            Self::PerKeyAndIp => "per_key_and_ip",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "per_key" => Some(Self::PerKey),
            "per_ip" => Some(Self::PerIp),
            "per_key_and_ip" => Some(Self::PerKeyAndIp),
            _ => None,
        }
    }
}

impl fmt::Display for RateLimitMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Default for RateLimitMode {
    fn default() -> Self {
        Self::PerKey
    }
}

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
    pub rate_limit_per_day: Option<i32>,
    #[sqlx(rename = "rate_limit_mode")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_mode: Option<RateLimitMode>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl ApiKeyApp {
    /// Get the effective rate limit mode (defaults to PerKey if not set)
    pub fn get_rate_limit_mode(&self) -> RateLimitMode {
        self.rate_limit_mode.unwrap_or_default()
    }
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
