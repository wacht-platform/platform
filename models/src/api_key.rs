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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "snake_case")]
pub enum RateLimitUnit {
    #[sqlx(rename = "second")]
    Second,
    #[sqlx(rename = "minute")]
    Minute,
    #[sqlx(rename = "hour")]
    Hour,
    #[sqlx(rename = "day")]
    Day,
}

impl RateLimitUnit {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Second => "second",
            Self::Minute => "minute",
            Self::Hour => "hour",
            Self::Day => "day",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "second" => Some(Self::Second),
            "minute" => Some(Self::Minute),
            "hour" => Some(Self::Hour),
            "day" => Some(Self::Day),
            _ => None,
        }
    }

    /// Convert duration in this unit to seconds
    pub fn to_seconds(&self, duration: i32) -> i64 {
        match self {
            Self::Second => duration as i64,
            Self::Minute => duration as i64 * 60,
            Self::Hour => duration as i64 * 3600,
            Self::Day => duration as i64 * 86400,
        }
    }
}

impl fmt::Display for RateLimitUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RateLimit {
    pub unit: RateLimitUnit,
    pub duration: i32,
    pub max_requests: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<RateLimitMode>,
}

impl RateLimit {
    /// Calculate the window in seconds for this rate limit
    pub fn window_seconds(&self) -> i64 {
        self.unit.to_seconds(self.duration)
    }

    /// Get the effective rate limit mode (defaults to PerKey if not set)
    pub fn effective_mode(&self) -> RateLimitMode {
        self.mode.unwrap_or_default()
    }

    /// Validate that the rate limit window is within supported bounds
    pub fn validate(&self) -> Result<(), String> {
        if self.duration <= 0 {
            return Err("Duration must be positive".to_string());
        }

        if self.max_requests <= 0 {
            return Err("Max requests must be positive".to_string());
        }

        let window_seconds = self.window_seconds();

        if window_seconds > 86400 {
            return Err("Rate limit window cannot exceed 24 hours (86400 seconds)".to_string());
        }

        match self.unit {
            RateLimitUnit::Second => {
                if self.duration > 1800 {
                    return Err(
                        "Second-based limits cannot exceed 1800 seconds (30 minutes)".to_string(),
                    );
                }
            }
            RateLimitUnit::Minute => {
                if self.duration > 1440 {
                    return Err(
                        "Minute-based limits cannot exceed 1440 minutes (24 hours)".to_string()
                    );
                }
            }
            RateLimitUnit::Hour => {
                if self.duration > 24 {
                    return Err("Hour-based limits cannot exceed 24 hours".to_string());
                }
            }
            RateLimitUnit::Day => {
                if self.duration != 1 {
                    return Err("Day-based limits must be exactly 1 day".to_string());
                }
            }
        }

        Ok(())
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
    #[sqlx(json)]
    pub rate_limits: Vec<RateLimit>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl ApiKeyApp {
    /// Get default rate limits if none are configured
    pub fn effective_rate_limits(&self) -> Vec<RateLimit> {
        if self.rate_limits.is_empty() {
            vec![RateLimit {
                unit: RateLimitUnit::Minute,
                duration: 1,
                max_requests: 100,
                mode: Some(RateLimitMode::PerKey),
            }]
        } else {
            self.rate_limits.clone()
        }
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

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKeyWithIdentifers {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub app_name: String,
    #[sqlx(json)]
    pub permissions: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub is_active: bool,
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

// Conversions from SDK types to model types
impl From<wacht::api::api_keys::ApiKeyApp> for ApiKeyApp {
    fn from(sdk_app: wacht::api::api_keys::ApiKeyApp) -> Self {
        let rate_limits = sdk_app
            .rate_limits
            .into_iter()
            .map(|sdk_limit| RateLimit {
                unit: match sdk_limit.unit {
                    wacht::api::api_keys::RateLimitUnit::Second => RateLimitUnit::Second,
                    wacht::api::api_keys::RateLimitUnit::Minute => RateLimitUnit::Minute,
                    wacht::api::api_keys::RateLimitUnit::Hour => RateLimitUnit::Hour,
                    wacht::api::api_keys::RateLimitUnit::Day => RateLimitUnit::Day,
                },
                duration: sdk_limit.duration,
                max_requests: sdk_limit.max_requests,
                mode: sdk_limit.mode.map(|m| match m {
                    wacht::api::api_keys::RateLimitMode::PerKey => RateLimitMode::PerKey,
                    wacht::api::api_keys::RateLimitMode::PerIp => RateLimitMode::PerIp,
                    wacht::api::api_keys::RateLimitMode::PerKeyAndIp => RateLimitMode::PerKeyAndIp,
                }),
            })
            .collect();

        Self {
            id: sdk_app.id.parse().unwrap_or(0),
            deployment_id: sdk_app.deployment_id.parse().unwrap_or(0),
            name: sdk_app.name,
            description: sdk_app.description,
            is_active: sdk_app.is_active,
            rate_limits,
            created_at: sdk_app.created_at,
            updated_at: sdk_app.updated_at,
            deleted_at: None,
        }
    }
}

impl From<wacht::api::api_keys::ApiKey> for ApiKey {
    fn from(sdk_key: wacht::api::api_keys::ApiKey) -> Self {
        Self {
            id: sdk_key.id.parse().unwrap_or(0),
            app_id: sdk_key.app_id.parse().unwrap_or(0),
            deployment_id: sdk_key.deployment_id.parse().unwrap_or(0),
            name: sdk_key.name,
            key_prefix: sdk_key.key_prefix,
            key_suffix: sdk_key.key_suffix,
            key_hash: String::new(), // SDK doesn't have key_hash for security
            permissions: sdk_key.permissions,
            metadata: sdk_key.metadata,
            expires_at: sdk_key.expires_at,
            last_used_at: sdk_key.last_used_at,
            is_active: sdk_key.is_active,
            created_at: sdk_key.created_at,
            updated_at: sdk_key.updated_at,
            revoked_at: sdk_key.revoked_at,
            revoked_reason: sdk_key.revoked_reason,
        }
    }
}

impl From<wacht::api::api_keys::ApiKeyWithSecret> for ApiKeyWithSecret {
    fn from(sdk_key: wacht::api::api_keys::ApiKeyWithSecret) -> Self {
        Self {
            key: sdk_key.key.into(),
            secret: sdk_key.secret,
        }
    }
}
