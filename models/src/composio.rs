use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComposioEnabledApp {
    pub slug: String,
    pub auth_config_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
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
    Managed,
    Custom {
        client_id: String,
        client_secret: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        scopes: Vec<String>,
    },
    UseExisting {
        auth_config_id: String,
    },
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
