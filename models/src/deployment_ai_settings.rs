use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStorageProvider {
    S3,
}

impl Default for DeploymentStorageProvider {
    fn default() -> Self {
        Self::S3
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentLlmProvider {
    Gemini,
    Openai,
    Openrouter,
}

impl Default for DeploymentLlmProvider {
    fn default() -> Self {
        Self::Gemini
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentEmbeddingProvider {
    Gemini,
    Openai,
    Openrouter,
}

impl Default for DeploymentEmbeddingProvider {
    fn default() -> Self {
        Self::Gemini
    }
}

pub const EMBEDDING_DIMENSION_1536: i32 = 1536;
pub const EMBEDDING_DIMENSION_768: i32 = 768;

pub fn default_embedding_dimension() -> i32 {
    EMBEDDING_DIMENSION_1536
}

pub fn is_supported_embedding_dimension(value: i32) -> bool {
    matches!(value, EMBEDDING_DIMENSION_1536 | EMBEDDING_DIMENSION_768)
}

pub fn default_embedding_provider() -> DeploymentEmbeddingProvider {
    DeploymentEmbeddingProvider::Gemini
}

pub fn default_embedding_model_for_provider(provider: &DeploymentEmbeddingProvider) -> String {
    match provider {
        DeploymentEmbeddingProvider::Gemini => "gemini-embedding-2-preview".to_string(),
        DeploymentEmbeddingProvider::Openai => "text-embedding-3-small".to_string(),
        DeploymentEmbeddingProvider::Openrouter => "openai/text-embedding-3-small".to_string(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentStorageSettingsResponse {
    pub provider: DeploymentStorageProvider,
    pub bucket: Option<String>,
    pub region: Option<String>,
    pub endpoint: Option<String>,
    pub root_prefix: Option<String>,
    pub force_path_style: bool,
    pub access_key_id_set: bool,
    pub secret_access_key_set: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateDeploymentStorageSettingsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<DeploymentStorageProvider>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_path_style: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_access_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DeploymentAiSettings {
    pub id: i64,
    pub deployment_id: i64,
    pub strong_llm_provider: String,
    pub weak_llm_provider: String,
    pub gemini_api_key: Option<String>,
    pub openrouter_api_key: Option<String>,
    pub openrouter_require_parameters: bool,
    pub openai_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub strong_model: Option<String>,
    pub weak_model: Option<String>,
    pub embedding_provider: String,
    pub embedding_model: String,
    pub embedding_dimension: i32,
    pub storage_provider: String,
    pub storage_bucket: Option<String>,
    pub storage_region: Option<String>,
    pub storage_endpoint: Option<String>,
    pub storage_root_prefix: Option<String>,
    pub storage_force_path_style: bool,
    pub storage_access_key_id: Option<String>,
    pub storage_secret_access_key: Option<String>,
    pub vector_store_initialized_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Response DTO that masks sensitive keys
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentAiSettingsResponse {
    pub strong_llm_provider: DeploymentLlmProvider,
    pub weak_llm_provider: DeploymentLlmProvider,
    pub gemini_api_key_set: bool,
    pub openrouter_api_key_set: bool,
    pub openrouter_require_parameters: bool,
    pub openai_api_key_set: bool,
    pub anthropic_api_key_set: bool,
    pub strong_model: Option<String>,
    pub weak_model: Option<String>,
    pub embedding_provider: DeploymentEmbeddingProvider,
    pub embedding_model: String,
    pub embedding_dimension: i32,
    pub storage: DeploymentStorageSettingsResponse,
}

impl From<DeploymentAiSettings> for DeploymentAiSettingsResponse {
    fn from(settings: DeploymentAiSettings) -> Self {
        Self {
            strong_llm_provider: match settings.strong_llm_provider.as_str() {
                "openai" => DeploymentLlmProvider::Openai,
                "openrouter" => DeploymentLlmProvider::Openrouter,
                _ => DeploymentLlmProvider::Gemini,
            },
            weak_llm_provider: match settings.weak_llm_provider.as_str() {
                "openai" => DeploymentLlmProvider::Openai,
                "openrouter" => DeploymentLlmProvider::Openrouter,
                _ => DeploymentLlmProvider::Gemini,
            },
            gemini_api_key_set: settings.gemini_api_key.is_some(),
            openrouter_api_key_set: settings.openrouter_api_key.is_some(),
            openrouter_require_parameters: settings.openrouter_require_parameters,
            openai_api_key_set: settings.openai_api_key.is_some(),
            anthropic_api_key_set: settings.anthropic_api_key.is_some(),
            strong_model: settings.strong_model,
            weak_model: settings.weak_model,
            embedding_provider: match settings.embedding_provider.as_str() {
                "openai" => DeploymentEmbeddingProvider::Openai,
                "openrouter" => DeploymentEmbeddingProvider::Openrouter,
                _ => DeploymentEmbeddingProvider::Gemini,
            },
            embedding_model: settings.embedding_model,
            embedding_dimension: settings.embedding_dimension,
            storage: DeploymentStorageSettingsResponse {
                provider: DeploymentStorageProvider::S3,
                bucket: settings.storage_bucket,
                region: settings.storage_region,
                endpoint: settings.storage_endpoint,
                root_prefix: settings.storage_root_prefix,
                force_path_style: settings.storage_force_path_style,
                access_key_id_set: settings.storage_access_key_id.is_some(),
                secret_access_key_set: settings.storage_secret_access_key.is_some(),
            },
        }
    }
}

/// Request DTO for updating AI settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateDeploymentAiSettingsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strong_llm_provider: Option<DeploymentLlmProvider>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weak_llm_provider: Option<DeploymentLlmProvider>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openrouter_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openrouter_require_parameters: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anthropic_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strong_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weak_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_provider: Option<DeploymentEmbeddingProvider>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_dimension: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<UpdateDeploymentStorageSettingsRequest>,
}
