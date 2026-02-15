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
    #[sqlx(rename = "per_app")]
    PerApp, // Rate limit is applied per app (all keys share the limit)
    #[sqlx(rename = "per_key")]
    PerKey, // Rate limit is applied per API key
    #[sqlx(rename = "per_key_and_ip")]
    PerKeyAndIp, // Rate limit is applied per combination of key and IP
    #[sqlx(rename = "per_app_and_ip")]
    PerAppAndIp, // Rate limit is applied per combination of app and IP
}

impl RateLimitMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PerApp => "per_app",
            Self::PerKey => "per_key",
            Self::PerKeyAndIp => "per_key_and_ip",
            Self::PerAppAndIp => "per_app_and_ip",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "per_app" => Some(Self::PerApp),
            "per_key" => Some(Self::PerKey),
            "per_key_and_ip" => Some(Self::PerKeyAndIp),
            "per_app_and_ip" => Some(Self::PerAppAndIp),
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
        Self::PerApp
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "snake_case")]
pub enum RateLimitUnit {
    #[sqlx(rename = "millisecond")]
    Millisecond,
    #[sqlx(rename = "second")]
    Second,
    #[sqlx(rename = "minute")]
    Minute,
    #[sqlx(rename = "hour")]
    Hour,
    #[sqlx(rename = "day")]
    Day,
    #[sqlx(rename = "calendar_day")]
    CalendarDay, // Per calendar day (resets at midnight UTC)
    #[sqlx(rename = "month")]
    Month, // Rolling 30-day window
    #[sqlx(rename = "calendar_month")]
    CalendarMonth, // Per calendar month (resets on 1st of month UTC)
}

impl RateLimitUnit {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Millisecond => "millisecond",
            Self::Second => "second",
            Self::Minute => "minute",
            Self::Hour => "hour",
            Self::Day => "day",
            Self::CalendarDay => "calendar_day",
            Self::Month => "month",
            Self::CalendarMonth => "calendar_month",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "millisecond" => Some(Self::Millisecond),
            "second" => Some(Self::Second),
            "minute" => Some(Self::Minute),
            "hour" => Some(Self::Hour),
            "day" => Some(Self::Day),
            "calendar_day" => Some(Self::CalendarDay),
            "month" => Some(Self::Month),
            "calendar_month" => Some(Self::CalendarMonth),
            _ => None,
        }
    }

    /// Convert duration in this unit to seconds
    pub fn to_seconds(&self, duration: i32) -> i64 {
        match self {
            Self::Millisecond => duration as i64 / 1000,
            Self::Second => duration as i64,
            Self::Minute => duration as i64 * 60,
            Self::Hour => duration as i64 * 3600,
            Self::Day => duration as i64 * 86400,
            Self::CalendarDay => duration as i64 * 86400,
            Self::Month => duration as i64 * 30 * 86400, // 30 days
            Self::CalendarMonth => duration as i64 * 30 * 86400, // ~30 days
        }
    }

    /// Whether this unit uses calendar-based reset (midnight/month boundary)
    pub fn is_calendar_based(&self) -> bool {
        matches!(self, Self::CalendarDay | Self::CalendarMonth)
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoints: Option<Vec<String>>, // ["*"] = all endpoints, or specific paths
    #[serde(default)]
    pub priority: i32, // Lower = higher precedence, defaults to 0
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

    /// Get the endpoints this rate limit applies to (defaults to ["*"] for all endpoints)
    pub fn effective_endpoints(&self) -> Vec<String> {
        self.endpoints
            .clone()
            .unwrap_or_else(|| vec!["*".to_string()])
    }

    /// Check if this rate limit matches the given endpoint
    pub fn matches_endpoint(&self, endpoint: &str) -> bool {
        let endpoints = self.effective_endpoints();
        endpoints.iter().any(|e| e == "*" || e == endpoint)
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

        // Allow up to 30 days for monthly limits
        if window_seconds > 30 * 86400 {
            return Err("Rate limit window cannot exceed 30 days".to_string());
        }

        match self.unit {
            RateLimitUnit::Millisecond => {
                if self.duration > 60000 {
                    return Err(
                        "Millisecond-based limits cannot exceed 60000 milliseconds (60 seconds)"
                            .to_string(),
                    );
                }
            }
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
                if self.duration > 720 {
                    return Err("Hour-based limits cannot exceed 720 hours (30 days)".to_string());
                }
            }
            RateLimitUnit::Day => {
                if self.duration > 30 {
                    return Err("Day-based limits cannot exceed 30 days".to_string());
                }
            }
            RateLimitUnit::CalendarDay => {
                if self.duration != 1 {
                    return Err("Calendar day limits must be exactly 1 day".to_string());
                }
            }
            RateLimitUnit::Month => {
                if self.duration != 1 {
                    return Err("Month-based limits must be exactly 1 month (30 days)".to_string());
                }
            }
            RateLimitUnit::CalendarMonth => {
                if self.duration != 1 {
                    return Err("Calendar month limits must be exactly 1 month".to_string());
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiAuthApp {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub app_slug: String, // TODO: remove id field and use (deployment_id, app_slug) as PK
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub key_prefix: String,
    #[sqlx(json)]
    pub rate_limits: Vec<RateLimit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_scheme_slug: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl ApiAuthApp {
    /// Get default rate limits if none are configured
    pub fn effective_rate_limits(&self) -> Vec<RateLimit> {
        if self.rate_limits.is_empty() {
            vec![RateLimit {
                unit: RateLimitUnit::Minute,
                duration: 1,
                max_requests: 100,
                mode: Some(RateLimitMode::PerKey),
                endpoints: None,
                priority: 0,
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
    pub app_id: i64, // TODO: remove when switching to app_slug only
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub app_slug: String, // TODO: remove app_id and use (deployment_id, app_slug) as FK
    pub name: String,
    pub key_prefix: String,
    pub key_suffix: String,
    #[serde(skip_serializing)]
    pub key_hash: String,
    #[sqlx(json)]
    pub permissions: Vec<String>,
    #[sqlx(json)]
    pub metadata: Value,
    #[sqlx(json)]
    pub rate_limits: Vec<RateLimit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_scheme_slug: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string"
    )]
    pub organization_id: Option<i64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string"
    )]
    pub workspace_id: Option<i64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string"
    )]
    pub organization_membership_id: Option<i64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string"
    )]
    pub workspace_membership_id: Option<i64>,
    #[sqlx(json)]
    pub org_role_permissions: Vec<String>,
    #[sqlx(json)]
    pub workspace_role_permissions: Vec<String>,
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
    pub app_slug: String,
    #[sqlx(json)]
    pub permissions: Vec<String>,
    #[sqlx(json)]
    pub org_role_permissions: Vec<String>,
    #[sqlx(json)]
    pub workspace_role_permissions: Vec<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string"
    )]
    pub organization_id: Option<i64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string"
    )]
    pub workspace_id: Option<i64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string"
    )]
    pub organization_membership_id: Option<i64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string"
    )]
    pub workspace_membership_id: Option<i64>,
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

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct RateLimitScheme {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    #[sqlx(json)]
    pub rules: Vec<RateLimit>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
