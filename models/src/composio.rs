use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComposioEnabledApp {
    pub slug: String,
    pub auth_config_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_scheme: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposioToolkit {
    pub slug: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub auth_schemes: Vec<String>,
    #[serde(default)]
    pub tool_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposioToolkitListResponse {
    pub toolkits: Vec<ComposioToolkit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Raw row slice selected from deployment_ai_settings for Composio.
#[derive(Debug, Clone)]
pub struct ComposioSettingsRow {
    pub enabled: bool,
    pub use_platform_key: bool,
    pub api_key: Option<String>,
    pub enabled_apps: serde_json::Value,
}

/// Response DTO — masks api_key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposioConfigResponse {
    pub enabled: bool,
    pub use_platform_key: bool,
    pub api_key_set: bool,
    pub enabled_apps: Vec<ComposioEnabledApp>,
}

impl From<ComposioSettingsRow> for ComposioConfigResponse {
    fn from(row: ComposioSettingsRow) -> Self {
        let enabled_apps: Vec<ComposioEnabledApp> =
            serde_json::from_value(row.enabled_apps).unwrap_or_default();
        Self {
            enabled: row.enabled,
            use_platform_key: row.use_platform_key,
            api_key_set: row.api_key.is_some(),
            enabled_apps,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateComposioConfigRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_platform_key: Option<bool>,
    /// `Some(Some(key))` = set, `Some(None)` = clear, `None` = leave unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_apps: Option<Vec<ComposioEnabledApp>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposioAuthConfigSummary {
    pub id: String,
    pub name: String,
    pub auth_scheme: Option<String>,
    pub is_composio_managed: bool,
    pub toolkit_slug: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposioAuthConfigListResponse {
    pub auth_configs: Vec<ComposioAuthConfigSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ComposioEnableAppAuth {
    Managed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth_scheme: Option<String>,
        #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
        credentials: serde_json::Map<String, serde_json::Value>,
    },
    Custom {
        auth_scheme: String,
        #[serde(default)]
        credentials: serde_json::Map<String, serde_json::Value>,
    },
    UseExisting {
        auth_config_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth_scheme: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposioToolkitAuthField {
    pub name: String,
    pub display_name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub description: String,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComposioToolkitAuthFields {
    #[serde(default)]
    pub required: Vec<ComposioToolkitAuthField>,
    #[serde(default)]
    pub optional: Vec<ComposioToolkitAuthField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposioToolkitAuthMode {
    pub mode: String,
    pub name: String,
    pub auth_config_creation: ComposioToolkitAuthFields,
    pub connected_account_initiation: ComposioToolkitAuthFields,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_hint_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposioToolkitDetailsResponse {
    pub slug: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    #[serde(default)]
    pub composio_managed_auth_schemes: Vec<String>,
    pub auth_modes: Vec<ComposioToolkitAuthMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnableComposioAppRequest {
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
    pub auth: ComposioEnableAppAuth,
}
