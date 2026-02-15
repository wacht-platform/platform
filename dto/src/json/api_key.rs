use chrono::{DateTime, Utc};
use models::api_key::{ApiAuthApp, ApiKey, RateLimit};
use models::api_key_permissions::{ApiKeyScope, ApiKeyScopeHelper};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyStatus {
    pub is_activated: bool,
    pub app: Option<ApiAuthApp>,
    pub keys: Option<Vec<ApiKey>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyStats {
    pub total_keys: i64,
    pub active_keys: i64,
    pub revoked_keys: i64,
    pub keys_used_24h: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyActivationResponse {
    pub app: ApiAuthApp,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct ListApiAuthAppsQuery {
    pub include_inactive: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ListApiAuthAppsResponse {
    pub apps: Vec<ApiAuthApp>,
    pub total: usize,
}

#[derive(Debug, Deserialize)]
pub struct CreateRateLimitSchemeRequest {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub rules: Vec<RateLimit>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRateLimitSchemeRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub rules: Option<Vec<RateLimit>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListRateLimitSchemesResponse<T> {
    pub schemes: Vec<T>,
    pub total: usize,
}

#[derive(Debug, Deserialize)]
pub struct CreateApiAuthAppRequest {
    pub app_slug: String,
    pub name: String,
    pub key_prefix: String,
    pub description: Option<String>,
    pub rate_limit_scheme_slug: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateApiAuthAppRequest {
    pub name: Option<String>,
    pub key_prefix: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
    pub rate_limit_scheme_slug: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListApiKeysQuery {
    pub include_inactive: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ListApiAuditLogsQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub outcome: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_string_or_number_to_option_i64"
    )]
    pub key_id: Option<i64>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub cursor: Option<String>,
    pub cursor_ts: Option<DateTime<Utc>>,
    pub cursor_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GetApiAuditAnalyticsQuery {
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    #[serde(
        default,
        deserialize_with = "deserialize_string_or_number_to_option_i64"
    )]
    pub key_id: Option<i64>,
    pub include_top_keys: Option<bool>,
    pub include_top_paths: Option<bool>,
    pub include_blocked_reasons: Option<bool>,
    pub include_rate_limits: Option<bool>,
    pub top_limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct GetApiAuditTimeseriesQuery {
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub interval: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_string_or_number_to_option_i64"
    )]
    pub key_id: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListApiKeysResponse {
    pub keys: Vec<ApiKey>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiAuditLog {
    pub request_id: String,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub app_slug: String,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub key_id: i64,
    pub key_name: String,
    pub outcome: String,
    pub blocked_by_rule: Option<String>,
    pub client_ip: String,
    pub path: String,
    pub user_agent: Option<String>,
    pub rate_limits: Option<Value>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiAuditLogsResponse {
    pub data: Vec<ApiAuditLog>,
    pub limit: u32,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiAuditAnalyticsResponse {
    pub total_requests: u64,
    pub allowed_requests: u64,
    pub blocked_requests: u64,
    pub success_rate: f64,
    pub keys_used_24h: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_keys: Option<Vec<ApiAuditTopKey>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_paths: Option<Vec<ApiAuditTopPath>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_reasons: Option<Vec<ApiAuditBlockedReason>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_stats: Option<ApiAuditRateLimitBreakdown>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiAuditTopKey {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub key_id: i64,
    pub key_name: String,
    pub total_requests: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiAuditTopPath {
    pub path: String,
    pub total_requests: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiAuditBlockedReason {
    pub blocked_by_rule: String,
    pub count: i64,
    pub percentage: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiAuditRateLimitBreakdown {
    pub total_hits: i64,
    pub percentage_of_blocked: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub top_rules: Vec<ApiAuditRateLimitRule>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiAuditRateLimitRule {
    pub rule: String,
    pub hit_count: i64,
    pub percentage: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiAuditTimeseriesPoint {
    pub timestamp: DateTime<Utc>,
    pub total_requests: i64,
    pub allowed_requests: i64,
    pub blocked_requests: i64,
    pub success_rate: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiAuditTimeseriesResponse {
    pub data: Vec<ApiAuditTimeseriesPoint>,
    pub interval: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub permissions: Option<Vec<String>>, // If omitted, default scopes are applied
    #[serde(
        default,
        deserialize_with = "deserialize_string_or_number_to_option_i64"
    )]
    pub organization_membership_id: Option<i64>,
    #[serde(
        default,
        deserialize_with = "deserialize_string_or_number_to_option_i64"
    )]
    pub workspace_membership_id: Option<i64>,
    pub metadata: Option<Value>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl CreateApiKeyRequest {
    /// Get the actual permissions based on provided permissions or defaults
    pub fn get_permissions(&self) -> Vec<String> {
        if let Some(perms) = &self.permissions {
            if !perms.is_empty() {
                return perms.clone();
            }
        }

        ApiKeyScopeHelper::scopes_to_strings(&ApiKeyScope::default_scopes())
    }
}

#[derive(Debug, Deserialize)]
pub struct RevokeApiKeyRequest {
    #[serde(
        default,
        deserialize_with = "deserialize_string_or_number_to_option_i64"
    )]
    pub key_id: Option<i64>, // Optional for backend API (passed in body)
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RotateApiKeyRequest {
    #[serde(deserialize_with = "deserialize_string_or_number_to_i64")]
    pub key_id: i64,
}

// Helper functions for deserializing string or number to i64
fn deserialize_string_or_number_to_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        String(String),
        Number(i64),
    }

    match StringOrNumber::deserialize(deserializer)? {
        StringOrNumber::String(s) => s.parse::<i64>().map_err(de::Error::custom),
        StringOrNumber::Number(n) => Ok(n),
    }
}

fn deserialize_string_or_number_to_option_i64<'de, D>(
    deserializer: D,
) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        String(String),
        Number(i64),
    }

    Option::<StringOrNumber>::deserialize(deserializer)?
        .map(|value| match value {
            StringOrNumber::String(s) => s.parse::<i64>().map_err(de::Error::custom),
            StringOrNumber::Number(n) => Ok(n),
        })
        .transpose()
}

#[derive(Debug, Serialize)]
pub struct ApiKeyScopeInfo {
    pub scope: String,
    pub description: String,
    pub category: String,
}

#[derive(Debug, Serialize)]
pub struct ScopePresetInfo {
    pub name: String,
    pub description: String,
    pub scopes: Vec<String>,
}
