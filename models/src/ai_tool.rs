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
    PlatformEvent,
    PlatformFunction,
    Internal,
    UseExternalService,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum AiToolConfiguration {
    Api(ApiToolConfiguration),
    PlatformEvent(PlatformEventToolConfiguration),
    PlatformFunction(PlatformFunctionToolConfiguration),
    Internal(InternalToolConfiguration),
    UseExternalService(UseExternalServiceToolConfiguration),
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

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct SchemaField {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub field_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub items_type: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InternalToolType {
    ReadFile,
    WriteFile,
    ListDirectory,
    SearchFiles,
    ExecuteCommand,
    Sleep,
    SwitchExecutionMode,
    UpdateTaskBoard,
    ExitSupervisorMode,
    SaveMemory,
    UpdateStatus,
    GetChildStatus,
    SpawnContext,
    SpawnControl,
    GetCompletionSummary,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct InternalToolConfiguration {
    pub tool_type: InternalToolType,
    pub input_schema: Option<Vec<SchemaField>>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UseExternalServiceToolType {
    TeamsListUsers,
    TeamsSearchUsers,
    TeamsSendContextMessage,
    TeamsListMessages,
    TeamsGetMeetingRecording,
    TeamsTranscribeMeeting,
    TeamsSaveAttachment,
    TeamsDescribeImage,
    TeamsTranscribeAudio,
    TeamsListContexts,
    SpawnContextExecution,
    #[serde(rename = "clickup_create_task")]
    ClickUpCreateTask,
    #[serde(rename = "clickup_create_list")]
    ClickUpCreateList,
    #[serde(rename = "clickup_update_task")]
    ClickUpUpdateTask,
    #[serde(rename = "clickup_add_comment")]
    ClickUpAddComment,
    #[serde(rename = "clickup_get_task")]
    ClickUpGetTask,
    #[serde(rename = "clickup_get_space_lists")]
    ClickUpGetSpaceLists,
    #[serde(rename = "clickup_get_spaces")]
    ClickUpGetSpaces,
    #[serde(rename = "clickup_get_teams")]
    ClickUpGetTeams,
    #[serde(rename = "clickup_get_current_user")]
    ClickUpGetCurrentUser,
    #[serde(rename = "clickup_get_tasks")]
    ClickUpGetTasks,
    #[serde(rename = "clickup_search_tasks")]
    ClickUpSearchTasks,
    #[serde(rename = "clickup_task_add_attachment")]
    ClickUpTaskAddAttachment,
    McpCallTool,
    WhatsAppSendMessage,
    WhatsAppGetMessage,
    WhatsAppMarkRead,
}

impl UseExternalServiceToolType {
    /// Get the integration type this tool belongs to
    pub fn integration_type(&self) -> Option<&'static str> {
        match self {
            UseExternalServiceToolType::TeamsListUsers
            | UseExternalServiceToolType::TeamsSearchUsers
            | UseExternalServiceToolType::TeamsListMessages
            | UseExternalServiceToolType::TeamsGetMeetingRecording
            | UseExternalServiceToolType::TeamsTranscribeMeeting
            | UseExternalServiceToolType::TeamsSaveAttachment
            | UseExternalServiceToolType::TeamsDescribeImage
            | UseExternalServiceToolType::TeamsTranscribeAudio
            | UseExternalServiceToolType::TeamsListContexts => Some("teams"),

            UseExternalServiceToolType::ClickUpCreateTask
            | UseExternalServiceToolType::ClickUpCreateList
            | UseExternalServiceToolType::ClickUpUpdateTask
            | UseExternalServiceToolType::ClickUpAddComment
            | UseExternalServiceToolType::ClickUpGetTask
            | UseExternalServiceToolType::ClickUpGetSpaceLists
            | UseExternalServiceToolType::ClickUpGetSpaces
            | UseExternalServiceToolType::ClickUpGetTeams
            | UseExternalServiceToolType::ClickUpGetCurrentUser
            | UseExternalServiceToolType::ClickUpGetTasks
            | UseExternalServiceToolType::ClickUpSearchTasks
            | UseExternalServiceToolType::ClickUpTaskAddAttachment => Some("clickup"),

            UseExternalServiceToolType::WhatsAppSendMessage
            | UseExternalServiceToolType::WhatsAppGetMessage
            | UseExternalServiceToolType::WhatsAppMarkRead => Some("whatsapp"),

            UseExternalServiceToolType::McpCallTool => Some("mcp"),

            UseExternalServiceToolType::SpawnContextExecution
            | UseExternalServiceToolType::TeamsSendContextMessage => None,
        }
    }

    /// Get all tool types for a given integration
    pub fn for_integration_type(integration_type: &str) -> Vec<Self> {
        match integration_type.to_lowercase().as_str() {
            "teams" => vec![
                UseExternalServiceToolType::TeamsListUsers,
                UseExternalServiceToolType::TeamsSearchUsers,
                UseExternalServiceToolType::TeamsListMessages,
                UseExternalServiceToolType::TeamsGetMeetingRecording,
                UseExternalServiceToolType::TeamsTranscribeMeeting,
                UseExternalServiceToolType::TeamsSaveAttachment,
                UseExternalServiceToolType::TeamsDescribeImage,
                UseExternalServiceToolType::TeamsTranscribeAudio,
                UseExternalServiceToolType::TeamsListContexts,
            ],
            "clickup" => vec![
                UseExternalServiceToolType::ClickUpCreateTask,
                UseExternalServiceToolType::ClickUpCreateList,
                UseExternalServiceToolType::ClickUpUpdateTask,
                UseExternalServiceToolType::ClickUpAddComment,
                UseExternalServiceToolType::ClickUpGetTask,
                UseExternalServiceToolType::ClickUpGetSpaceLists,
                UseExternalServiceToolType::ClickUpGetSpaces,
                UseExternalServiceToolType::ClickUpGetTeams,
                UseExternalServiceToolType::ClickUpGetCurrentUser,
                UseExternalServiceToolType::ClickUpGetTasks,
                UseExternalServiceToolType::ClickUpSearchTasks,
                UseExternalServiceToolType::ClickUpTaskAddAttachment,
            ],
            "whatsapp" => vec![
                UseExternalServiceToolType::WhatsAppSendMessage,
                UseExternalServiceToolType::WhatsAppGetMessage,
                UseExternalServiceToolType::WhatsAppMarkRead,
            ],
            "mcp" => vec![UseExternalServiceToolType::McpCallTool],
            _ => vec![],
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UseExternalServiceToolConfiguration {
    pub service_type: UseExternalServiceToolType,
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
            "platform_function" => AiToolType::PlatformFunction,
            "internal" => AiToolType::Internal,
            "use_external_service" => AiToolType::UseExternalService,
            _ => AiToolType::Api,
        }
    }
}

impl From<AiToolType> for String {
    fn from(tool_type: AiToolType) -> Self {
        match tool_type {
            AiToolType::Api => "api".to_string(),
            AiToolType::PlatformEvent => "platform_event".to_string(),
            AiToolType::PlatformFunction => "platform_function".to_string(),
            AiToolType::Internal => "internal".to_string(),
            AiToolType::UseExternalService => "use_external_service".to_string(),
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
