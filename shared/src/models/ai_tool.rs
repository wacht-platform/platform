use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct AiTool {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub tool_type: AiToolType,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub configuration: AiToolConfiguration,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AiToolWithDetails {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub tool_type: AiToolType,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub configuration: AiToolConfiguration,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AiToolType {
    Api,
    KnowledgeBase,
    PlatformEvent,
    PlatformFunction,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum AiToolConfiguration {
    Api(ApiToolConfiguration),
    KnowledgeBase(KnowledgeBaseToolConfiguration),
    PlatformEvent(PlatformEventToolConfiguration),
    PlatformFunction(PlatformFunctionToolConfiguration),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ApiToolConfiguration {
    pub endpoint: String,
    pub method: HttpMethod,
    pub authorization: Option<AuthorizationConfiguration>,
    pub request_body_schema: Option<Vec<SchemaField>>,
    pub url_params_schema: Option<Vec<SchemaField>>,
    pub timeout_seconds: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KnowledgeBaseToolConfiguration {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub knowledge_base_id: i64,
    pub search_settings: KnowledgeBaseSearchSettings,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KnowledgeBaseSearchSettings {
    pub max_results: Option<u32>,
    pub similarity_threshold: Option<f32>,
    pub include_metadata: bool,
    pub sort_by_relevance: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PlatformEventToolConfiguration {
    pub event_label: String,
    pub event_data: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PlatformFunctionToolConfiguration {
    pub function_name: String,
    pub function_description: Option<String>,
    pub input_schema: Option<Vec<SchemaField>>,
    pub output_schema: Option<Vec<SchemaField>>,
    pub is_overridable: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SchemaField {
    pub name: String,
    pub field_type: String,
    pub required: bool,
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AuthorizationConfiguration {
    pub authorize_as_user: bool,
    pub jwt_template_id: Option<i64>,
    pub custom_headers: Option<Vec<SchemaField>>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
}

impl From<String> for AiToolType {
    fn from(tool_type: String) -> Self {
        match tool_type.as_str() {
            "api" => AiToolType::Api,
            "knowledge_base" => AiToolType::KnowledgeBase,
            "platform_event" => AiToolType::PlatformEvent,
            "platform_function" => AiToolType::PlatformFunction,
            _ => AiToolType::Api,
        }
    }
}

impl From<AiToolType> for String {
    fn from(tool_type: AiToolType) -> Self {
        match tool_type {
            AiToolType::Api => "api".to_string(),
            AiToolType::KnowledgeBase => "knowledge_base".to_string(),
            AiToolType::PlatformEvent => "platform_event".to_string(),
            AiToolType::PlatformFunction => "platform_function".to_string(),
        }
    }
}

impl From<String> for HttpMethod {
    fn from(method: String) -> Self {
        match method.to_uppercase().as_str() {
            "GET" => HttpMethod::GET,
            "POST" => HttpMethod::POST,
            "PUT" => HttpMethod::PUT,
            "DELETE" => HttpMethod::DELETE,
            "PATCH" => HttpMethod::PATCH,
            _ => HttpMethod::GET,
        }
    }
}

impl From<HttpMethod> for String {
    fn from(method: HttpMethod) -> Self {
        match method {
            HttpMethod::GET => "GET".to_string(),
            HttpMethod::POST => "POST".to_string(),
            HttpMethod::PUT => "PUT".to_string(),
            HttpMethod::DELETE => "DELETE".to_string(),
            HttpMethod::PATCH => "PATCH".to_string(),
        }
    }
}

impl Default for KnowledgeBaseSearchSettings {
    fn default() -> Self {
        Self {
            max_results: Some(10),
            similarity_threshold: Some(0.7),
            include_metadata: true,
            sort_by_relevance: true,
        }
    }
}

impl Default for AiToolConfiguration {
    fn default() -> Self {
        AiToolConfiguration::Api(ApiToolConfiguration {
            endpoint: String::new(),
            method: HttpMethod::GET,
            authorization: None,
            request_body_schema: None,
            url_params_schema: None,
            timeout_seconds: Some(30),
        })
    }
}

impl Default for ApiToolConfiguration {
    fn default() -> Self {
        Self {
            endpoint: "".to_string(),
            method: HttpMethod::GET,
            authorization: None,
            request_body_schema: None,
            url_params_schema: None,
            timeout_seconds: Some(30),
        }
    }
}

impl Default for KnowledgeBaseToolConfiguration {
    fn default() -> Self {
        Self {
            knowledge_base_id: 0,
            search_settings: KnowledgeBaseSearchSettings::default(),
        }
    }
}

impl Default for PlatformFunctionToolConfiguration {
    fn default() -> Self {
        Self {
            function_name: "".to_string(),
            function_description: None,
            input_schema: None,
            output_schema: None,
            is_overridable: true,
        }
    }
}
