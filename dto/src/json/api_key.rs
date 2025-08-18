use serde::{Deserialize, Serialize};
use serde_json::Value;
use chrono::{DateTime, Utc};
use models::api_key::{ApiKeyApp, ApiKey};

// =====================================================
// API KEY STATUS & STATS
// =====================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyStatus {
    pub is_activated: bool,
    pub app: Option<ApiKeyApp>,
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
    pub app: ApiKeyApp,
    pub message: String,
}

// =====================================================
// API KEY APP REQUESTS
// =====================================================

#[derive(Debug, Deserialize)]
pub struct ListApiKeyAppsQuery {
    pub include_inactive: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ListApiKeyAppsResponse {
    pub apps: Vec<ApiKeyApp>,
    pub total: usize,
}

#[derive(Debug, Deserialize)]
pub struct CreateApiKeyAppRequest {
    pub name: String,
    pub description: Option<String>,
    pub rate_limit_per_minute: Option<i32>,
    pub rate_limit_per_hour: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateApiKeyAppRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
    pub rate_limit_per_minute: Option<i32>,
    pub rate_limit_per_hour: Option<i32>,
}

// =====================================================
// API KEY REQUESTS
// =====================================================

#[derive(Debug, Deserialize)]
pub struct ListApiKeysQuery {
    pub include_inactive: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListApiKeysResponse {
    pub keys: Vec<ApiKey>,
}

#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub key_prefix: Option<String>, // Optional for console API (auto-determined), required for backend API
    pub permissions: Option<Vec<String>>,
    pub metadata: Option<Value>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct RevokeApiKeyRequest {
    pub key_id: Option<i64>, // Optional for backend API (passed in body)
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RotateApiKeyRequest {
    pub key_id: i64,
}