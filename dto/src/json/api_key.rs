use super::flexible_i64::FlexibleI64;
use chrono::{DateTime, Utc};
use models::api_key::{ApiAuthApp, ApiKey, JwksDocument, OAuthScopeDefinition, RateLimit};
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
    pub user_id: Option<FlexibleI64>,
    pub organization_id: Option<FlexibleI64>,
    pub workspace_id: Option<FlexibleI64>,
    pub app_slug: String,
    pub name: String,
    pub key_prefix: String,
    pub description: Option<String>,
    pub rate_limit_scheme_slug: Option<String>,
    pub permissions: Option<Vec<String>>,
    pub resources: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateApiAuthAppRequest {
    pub organization_id: Option<FlexibleI64>,
    pub workspace_id: Option<FlexibleI64>,
    pub name: Option<String>,
    pub key_prefix: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
    pub rate_limit_scheme_slug: Option<String>,
    pub permissions: Option<Vec<String>>,
    pub resources: Option<Vec<String>>,
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
    pub key_id: Option<FlexibleI64>,
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
    pub key_id: Option<FlexibleI64>,
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
    pub key_id: Option<FlexibleI64>,
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
    pub permissions: Option<Vec<String>>,
    pub metadata: Option<Value>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct RevokeApiKeyRequest {
    pub key_id: Option<FlexibleI64>, // Optional for backend API (passed in body)
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RotateApiKeyRequest {
    pub key_id: FlexibleI64,
}

#[derive(Debug, Serialize)]
pub struct OAuthAppResponse {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub logo_url: Option<String>,
    pub fqdn: String,
    pub supported_scopes: Vec<String>,
    pub scope_definitions: Vec<OAuthScopeDefinition>,
    pub allow_dynamic_client_registration: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ListOAuthAppsResponse {
    pub apps: Vec<OAuthAppResponse>,
}

#[derive(Debug, Deserialize)]
pub struct CreateOAuthClientRequest {
    pub client_auth_method: String,
    pub grant_types: Vec<String>,
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub logo_uri: Option<String>,
    pub tos_uri: Option<String>,
    pub policy_uri: Option<String>,
    pub contacts: Option<Vec<String>>,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
    pub token_endpoint_auth_signing_alg: Option<String>,
    pub jwks_uri: Option<String>,
    pub jwks: Option<JwksDocument>,
    pub public_key_pem: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateOAuthClientRequest {
    pub client_auth_method: Option<String>,
    pub grant_types: Option<Vec<String>>,
    pub redirect_uris: Option<Vec<String>>,
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub logo_uri: Option<String>,
    pub tos_uri: Option<String>,
    pub policy_uri: Option<String>,
    pub contacts: Option<Vec<String>>,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
    pub token_endpoint_auth_signing_alg: Option<String>,
    pub jwks_uri: Option<String>,
    pub jwks: Option<JwksDocument>,
    pub public_key_pem: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateOAuthAppRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub supported_scopes: Option<Vec<String>>,
    pub scope_definitions: Option<Vec<OAuthScopeDefinition>>,
    pub allow_dynamic_client_registration: Option<bool>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateOAuthScopeRequest {
    pub display_name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetOAuthScopeMappingRequest {
    pub category: String,
    pub organization_permission: Option<String>,
    pub workspace_permission: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OAuthClientResponse {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub oauth_app_id: i64,
    pub client_id: String,
    pub client_auth_method: String,
    pub grant_types: Vec<String>,
    pub redirect_uris: Vec<String>,
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub logo_uri: Option<String>,
    pub tos_uri: Option<String>,
    pub policy_uri: Option<String>,
    pub contacts: Vec<String>,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
    pub token_endpoint_auth_signing_alg: Option<String>,
    pub jwks_uri: Option<String>,
    pub jwks: Option<JwksDocument>,
    pub public_key_pem: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListOAuthClientsResponse {
    pub clients: Vec<OAuthClientResponse>,
}

#[derive(Debug, Serialize)]
pub struct RotateOAuthClientSecretResponse {
    pub client_secret: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateOAuthGrantRequest {
    pub resource: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct OAuthGrantResponse {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub id: i64,
    pub api_auth_app_slug: String,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub oauth_client_id: i64,
    pub resource: String,
    pub scopes: Vec<String>,
    pub status: String,
    pub granted_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "models::utils::serde::serialize_option_i64_as_string"
    )]
    pub granted_by_user_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ListOAuthGrantsResponse {
    pub grants: Vec<OAuthGrantResponse>,
}
