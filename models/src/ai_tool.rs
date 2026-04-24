use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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
    #[serde(default)]
    pub requires_user_approval: bool,
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
    #[serde(default)]
    pub requires_user_approval: bool,
    pub configuration: AiToolConfiguration,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AiToolType {
    Api,
    PlatformEvent,
    CodeRunner,
    Internal,
    Mcp,
    Virtual,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum AiToolConfiguration {
    Api(ApiToolConfiguration),
    PlatformEvent(PlatformEventToolConfiguration),
    CodeRunner(CodeRunnerToolConfiguration),
    Internal(InternalToolConfiguration),
    Mcp(McpToolConfiguration),
    Virtual(VirtualToolConfiguration),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct McpToolConfiguration {
    pub mcp_server_id: i64,
    pub remote_tool_name: String,
    pub input_schema: Option<serde_json::Value>,
}

/// Virtual tool: a runtime-only reference into a third-party integration
/// provider (Composio, Arcade, Pipedream, ...). Never persisted to the DB;
/// instances are constructed at runtime from the provider's search API and
/// held in the agent context. Kept distinct from a future "external" type
/// which would cover typed, DB-backed external-service integrations.
#[derive(Serialize, Deserialize, Clone)]
pub struct VirtualToolConfiguration {
    /// Provider slug (e.g. `composio`).
    pub provider: String,
    /// Provider-specific toolkit slug (e.g. `gmail`).
    pub toolkit_slug: String,
    /// Provider-specific tool slug (e.g. `GMAIL_SEND_EMAIL`).
    pub remote_tool_slug: String,
    /// Raw JSON Schema for the tool's arguments.
    pub input_schema: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ApiToolConfiguration {
    pub endpoint: String,
    #[serde(default)]
    pub method: HttpMethod,
    pub authorization: Option<AuthorizationConfiguration>,
    pub request_body_schema: Option<Vec<SchemaField>>,
    pub url_params_schema: Option<Vec<SchemaField>>,
    pub timeout_seconds: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PlatformEventToolConfiguration {
    pub event_label: String,
    pub event_data: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CodeRunnerRuntime {
    Python,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CodeRunnerToolConfiguration {
    #[serde(default)]
    pub runtime: CodeRunnerRuntime,
    pub code: String,
    pub input_schema: Option<Vec<SchemaField>>,
    pub output_schema: Option<Vec<SchemaField>>,
    #[serde(default)]
    pub env_variables: Option<Vec<CodeRunnerEnvVariable>>,
    pub timeout_seconds: Option<u32>,
    #[serde(default)]
    pub allow_network: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CodeRunnerEnvVariable {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SchemaField {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub field_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub enum_values: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub minimum: Option<f64>,
    #[serde(default)]
    pub maximum: Option<f64>,
    #[serde(default)]
    pub items_type: Option<String>,
    #[serde(default)]
    pub items_schema: Option<Box<SchemaField>>,
    #[serde(default)]
    pub min_items: Option<usize>,
    #[serde(default)]
    pub max_items: Option<usize>,
    #[serde(default)]
    pub properties: Option<Vec<SchemaField>>,
}

impl SchemaField {
    pub fn to_json_schema(&self) -> Value {
        let field_type = match self.field_type.as_str() {
            "STRING" => "string",
            "INTEGER" => "integer",
            "NUMBER" => "number",
            "BOOLEAN" => "boolean",
            "ARRAY" => "array",
            "OBJECT" => "object",
            _ => "string",
        };

        let mut schema = serde_json::Map::new();
        schema.insert("type".to_string(), json!(field_type));
        if let Some(title) = &self.title {
            schema.insert("title".to_string(), json!(title));
        }
        if let Some(description) = &self.description {
            schema.insert("description".to_string(), json!(description));
        }
        if let Some(enum_values) = &self.enum_values {
            schema.insert("enum".to_string(), json!(enum_values));
        }
        if let Some(format) = &self.format {
            schema.insert("format".to_string(), json!(format));
        }
        if let Some(minimum) = self.minimum {
            schema.insert("minimum".to_string(), json!(minimum));
        }
        if let Some(maximum) = self.maximum {
            schema.insert("maximum".to_string(), json!(maximum));
        }

        if field_type == "array" {
            if let Some(items_type) = &self.items_type {
                let item_type = match items_type.as_str() {
                    "STRING" => "string",
                    "INTEGER" => "integer",
                    "NUMBER" => "number",
                    "BOOLEAN" => "boolean",
                    "OBJECT" => "object",
                    _ => "string",
                };
                schema.insert("items".to_string(), json!({ "type": item_type }));
            } else if let Some(items_schema) = &self.items_schema {
                schema.insert("items".to_string(), items_schema.to_json_schema());
            }
            if let Some(min_items) = self.min_items {
                schema.insert("minItems".to_string(), json!(min_items));
            }
            if let Some(max_items) = self.max_items {
                schema.insert("maxItems".to_string(), json!(max_items));
            }
        } else if field_type == "object" {
            let object_schema = Self::object_json_schema(self.properties.as_deref().unwrap_or(&[]));
            if let Some(properties) = object_schema.get("properties") {
                schema.insert("properties".to_string(), properties.clone());
            }
            if let Some(required) = object_schema.get("required") {
                schema.insert("required".to_string(), required.clone());
            }
            if let Some(additional_properties) = object_schema.get("additionalProperties") {
                schema.insert(
                    "additionalProperties".to_string(),
                    additional_properties.clone(),
                );
            }
        }

        Value::Object(schema)
    }

    pub fn object_json_schema(fields: &[SchemaField]) -> Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for field in fields {
            if field.required {
                required.push(field.name.clone());
            }
            properties.insert(field.name.clone(), field.to_json_schema());
        }

        let mut schema = serde_json::Map::new();
        schema.insert("type".to_string(), json!("object"));
        schema.insert("properties".to_string(), Value::Object(properties));
        schema.insert("additionalProperties".to_string(), Value::Bool(false));
        if !required.is_empty() {
            schema.insert("required".to_string(), json!(required));
        }
        Value::Object(schema)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InternalToolType {
    ReadImage,
    ReadFile,
    WriteFile,
    EditFile,
    ExecuteCommand,
    Sleep,
    WebSearch,
    UrlContent,
    SearchKnowledgebase,
    LoadMemory,
    SearchTools,
    LoadTools,
    CreateProjectTask,
    UpdateProjectTask,
    AssignProjectTask,
    AppendTaskJournal,
    ListThreads,
    CreateThread,
    UpdateThread,
    SaveMemory,
    UpdateMemory,
    TaskGraphAddNode,
    TaskGraphAddDependency,
    TaskGraphMarkInProgress,
    TaskGraphCompleteNode,
    TaskGraphFailNode,
    TaskGraphReset,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct InternalToolConfiguration {
    pub tool_type: InternalToolType,
    pub input_schema: Option<Vec<SchemaField>>,
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
            "platform_event" => AiToolType::PlatformEvent,
            "code_runner" => AiToolType::CodeRunner,
            "internal" => AiToolType::Internal,
            "mcp" => AiToolType::Mcp,
            "virtual" => AiToolType::Virtual,
            _ => AiToolType::Api,
        }
    }
}

impl From<AiToolType> for String {
    fn from(tool_type: AiToolType) -> Self {
        match tool_type {
            AiToolType::Api => "api".to_string(),
            AiToolType::PlatformEvent => "platform_event".to_string(),
            AiToolType::CodeRunner => "code_runner".to_string(),
            AiToolType::Internal => "internal".to_string(),
            AiToolType::Mcp => "mcp".to_string(),
            AiToolType::Virtual => "virtual".to_string(),
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

impl Default for HttpMethod {
    fn default() -> Self {
        Self::GET
    }
}

impl Default for CodeRunnerRuntime {
    fn default() -> Self {
        Self::Python
    }
}

impl Default for CodeRunnerToolConfiguration {
    fn default() -> Self {
        Self {
            runtime: CodeRunnerRuntime::Python,
            code: String::new(),
            input_schema: None,
            output_schema: None,
            env_variables: None,
            timeout_seconds: Some(30),
            allow_network: false,
        }
    }
}
