use chrono::{DateTime, Utc};
use models::api_key::{ApiKey, ApiKeyApp};
use models::api_key_permissions::{ApiKeyScope, ApiKeyScopeHelper};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    pub rate_limit_per_day: Option<i32>,
    pub rate_limit_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateApiKeyAppRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
    pub rate_limit_per_minute: Option<i32>,
    pub rate_limit_per_hour: Option<i32>,
    pub rate_limit_per_day: Option<i32>,
    pub rate_limit_mode: Option<String>,
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
#[serde(rename_all = "snake_case")]
pub enum ApiKeyScopePreset {
    Default,   // Basic read-only access
    ReadOnly,  // Full read-only access
    ReadWrite, // Read and write, no delete
    Admin,     // Full admin access
    Custom,    // Custom permissions list
}

#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub key_prefix: Option<String>, // Optional for console API (auto-determined), required for backend API
    pub scope_preset: Option<ApiKeyScopePreset>, // Use a preset or custom
    pub permissions: Option<Vec<String>>, // Used when scope_preset is Custom or None
    pub metadata: Option<Value>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl CreateApiKeyRequest {
    /// Get the actual permissions based on preset or custom permissions
    pub fn get_permissions(&self) -> Vec<String> {
        match &self.scope_preset {
            Some(ApiKeyScopePreset::Default) | None => {
                ApiKeyScopeHelper::scopes_to_strings(&ApiKeyScope::default_scopes())
            }
            Some(ApiKeyScopePreset::ReadOnly) => {
                ApiKeyScopeHelper::scopes_to_strings(&ApiKeyScope::readonly_scopes())
            }
            Some(ApiKeyScopePreset::ReadWrite) => {
                ApiKeyScopeHelper::scopes_to_strings(&ApiKeyScope::readwrite_scopes())
            }
            Some(ApiKeyScopePreset::Admin) => {
                vec![ApiKeyScope::AdminAccess.as_str().to_string()]
            }
            Some(ApiKeyScopePreset::Custom) => self.permissions.clone().unwrap_or_else(|| {
                ApiKeyScopeHelper::scopes_to_strings(&ApiKeyScope::default_scopes())
            }),
        }
    }
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

// =====================================================
// API KEY SCOPE INFORMATION
// =====================================================

#[derive(Debug, Serialize)]
pub struct ApiKeyScopeInfo {
    pub scope: String,
    pub description: String,
    pub category: String,
}

#[derive(Debug, Serialize)]
pub struct AvailableScopesResponse {
    pub scopes: Vec<ApiKeyScopeInfo>,
    pub presets: Vec<ScopePresetInfo>,
}

#[derive(Debug, Serialize)]
pub struct ScopePresetInfo {
    pub name: String,
    pub description: String,
    pub scopes: Vec<String>,
}

impl AvailableScopesResponse {
    pub fn new() -> Self {
        let scopes = vec![
            // User Management
            ApiKeyScopeInfo {
                scope: "users:read".to_string(),
                description: "Read user information".to_string(),
                category: "User Management".to_string(),
            },
            ApiKeyScopeInfo {
                scope: "users:write".to_string(),
                description: "Create and update users".to_string(),
                category: "User Management".to_string(),
            },
            ApiKeyScopeInfo {
                scope: "users:delete".to_string(),
                description: "Delete users".to_string(),
                category: "User Management".to_string(),
            },
            // Organizations
            ApiKeyScopeInfo {
                scope: "organizations:read".to_string(),
                description: "Read organization information".to_string(),
                category: "Organizations".to_string(),
            },
            ApiKeyScopeInfo {
                scope: "organizations:write".to_string(),
                description: "Create and update organizations".to_string(),
                category: "Organizations".to_string(),
            },
            // Add more as needed...
        ];

        let presets = vec![
            ScopePresetInfo {
                name: "default".to_string(),
                description: "Basic read-only access to essential resources".to_string(),
                scopes: ApiKeyScopeHelper::scopes_to_strings(&ApiKeyScope::default_scopes()),
            },
            ScopePresetInfo {
                name: "read_only".to_string(),
                description: "Full read-only access to all resources".to_string(),
                scopes: ApiKeyScopeHelper::scopes_to_strings(&ApiKeyScope::readonly_scopes()),
            },
            ScopePresetInfo {
                name: "read_write".to_string(),
                description: "Read and write access (no delete permissions)".to_string(),
                scopes: ApiKeyScopeHelper::scopes_to_strings(&ApiKeyScope::readwrite_scopes()),
            },
            ScopePresetInfo {
                name: "admin".to_string(),
                description: "Full administrative access to all resources".to_string(),
                scopes: vec![ApiKeyScope::AdminAccess.as_str().to_string()],
            },
        ];

        Self { scopes, presets }
    }
}
