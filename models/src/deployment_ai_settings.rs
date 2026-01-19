use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DeploymentAiSettings {
    pub id: i64,
    pub deployment_id: i64,
    pub gemini_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Response DTO that masks sensitive keys
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentAiSettingsResponse {
    pub gemini_api_key_set: bool,
    pub openai_api_key_set: bool,
    pub anthropic_api_key_set: bool,
}

impl From<DeploymentAiSettings> for DeploymentAiSettingsResponse {
    fn from(settings: DeploymentAiSettings) -> Self {
        Self {
            gemini_api_key_set: settings.gemini_api_key.is_some(),
            openai_api_key_set: settings.openai_api_key.is_some(),
            anthropic_api_key_set: settings.anthropic_api_key.is_some(),
        }
    }
}

/// Request DTO for updating AI settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateDeploymentAiSettingsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anthropic_api_key: Option<String>,
}
