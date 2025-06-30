use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentExecutionContext {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub title: String,
    pub current_goal: String,
    pub tasks: Vec<String>,
    pub last_activity_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentExecutionContextMessage {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub execution_context_id: i64,
    pub message_type: ExecutionMessageType,
    pub sender: ExecutionMessageSender,
    pub content: String,
    pub metadata: serde_json::Value,
    pub tool_calls: Option<serde_json::Value>,
    pub tool_results: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ExecutionContextStatus {
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "waiting_for_input")]
    WaitingForInput,
    #[serde(rename = "interrupted")]
    Interrupted,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ExecutionMessageType {
    #[serde(rename = "user_input")]
    UserInput,
    #[serde(rename = "agent_response")]
    AgentResponse,
    #[serde(rename = "tool_call")]
    ToolCall,
    #[serde(rename = "tool_result")]
    ToolResult,
    #[serde(rename = "system_message")]
    SystemMessage,
    #[serde(rename = "error")]
    Error,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ExecutionMessageSender {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "agent")]
    Agent,
    #[serde(rename = "system")]
    System,
    #[serde(rename = "tool")]
    Tool,
}

// Redis stream message structure
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentStreamMessage {
    pub execution_context_id: i64,
    pub message_type: String,
    pub content: String,
    pub metadata: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

// WebSocket message types for agent execution
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentExecutionUpdate {
    pub execution_context_id: i64,
    pub message_type: ExecutionMessageType,
    pub content: String,
    pub metadata: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentExecutionRequest {
    pub agent_name: String,
    pub user_input: String,
    pub session_id: Option<String>,
    pub execution_context_id: Option<i64>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentExecutionResponse {
    pub execution_context_id: i64,
    pub status: ExecutionContextStatus,
    pub message: String,
    pub metadata: serde_json::Value,
}

// Context engine operations
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextStoreRequest {
    pub key: String,
    pub data: serde_json::Value,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextFetchRequest {
    pub key: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextSearchRequest {
    pub query: String,
    pub max_results: Option<usize>,
    pub metadata: Option<serde_json::Value>,
}

// Implementation helpers
impl ExecutionContextStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::WaitingForInput)
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

impl ExecutionMessageType {
    pub fn is_user_facing(&self) -> bool {
        matches!(self, Self::UserInput | Self::AgentResponse | Self::Error)
    }

    pub fn is_system(&self) -> bool {
        matches!(
            self,
            Self::ToolCall | Self::ToolResult | Self::SystemMessage
        )
    }
}

impl Default for ExecutionContextStatus {
    fn default() -> Self {
        Self::Idle
    }
}

impl Default for ExecutionMessageType {
    fn default() -> Self {
        Self::SystemMessage
    }
}

impl Default for ExecutionMessageSender {
    fn default() -> Self {
        Self::System
    }
}

// String conversions for database storage
impl From<ExecutionContextStatus> for String {
    fn from(status: ExecutionContextStatus) -> Self {
        match status {
            ExecutionContextStatus::Idle => "idle".to_string(),
            ExecutionContextStatus::Running => "running".to_string(),
            ExecutionContextStatus::WaitingForInput => "waiting_for_input".to_string(),
            ExecutionContextStatus::Interrupted => "interrupted".to_string(),
            ExecutionContextStatus::Completed => "completed".to_string(),
            ExecutionContextStatus::Failed => "failed".to_string(),
        }
    }
}

impl From<String> for ExecutionContextStatus {
    fn from(status: String) -> Self {
        match status.as_str() {
            "idle" => ExecutionContextStatus::Idle,
            "running" => ExecutionContextStatus::Running,
            "waiting_for_input" => ExecutionContextStatus::WaitingForInput,
            "interrupted" => ExecutionContextStatus::Interrupted,
            "completed" => ExecutionContextStatus::Completed,
            "failed" => ExecutionContextStatus::Failed,
            _ => ExecutionContextStatus::Idle,
        }
    }
}

impl From<ExecutionMessageType> for String {
    fn from(msg_type: ExecutionMessageType) -> Self {
        match msg_type {
            ExecutionMessageType::UserInput => "user_input".to_string(),
            ExecutionMessageType::AgentResponse => "agent_response".to_string(),
            ExecutionMessageType::ToolCall => "tool_call".to_string(),
            ExecutionMessageType::ToolResult => "tool_result".to_string(),
            ExecutionMessageType::SystemMessage => "system_message".to_string(),
            ExecutionMessageType::Error => "error".to_string(),
        }
    }
}

impl From<String> for ExecutionMessageType {
    fn from(msg_type: String) -> Self {
        match msg_type.as_str() {
            "user_input" => ExecutionMessageType::UserInput,
            "agent_response" => ExecutionMessageType::AgentResponse,
            "tool_call" => ExecutionMessageType::ToolCall,
            "tool_result" => ExecutionMessageType::ToolResult,
            "system_message" => ExecutionMessageType::SystemMessage,
            "error" => ExecutionMessageType::Error,
            _ => ExecutionMessageType::SystemMessage,
        }
    }
}

impl From<ExecutionMessageSender> for String {
    fn from(sender: ExecutionMessageSender) -> Self {
        match sender {
            ExecutionMessageSender::User => "user".to_string(),
            ExecutionMessageSender::Agent => "agent".to_string(),
            ExecutionMessageSender::System => "system".to_string(),
            ExecutionMessageSender::Tool => "tool".to_string(),
        }
    }
}

impl From<String> for ExecutionMessageSender {
    fn from(sender: String) -> Self {
        match sender.as_str() {
            "user" => ExecutionMessageSender::User,
            "agent" => ExecutionMessageSender::Agent,
            "system" => ExecutionMessageSender::System,
            "tool" => ExecutionMessageSender::Tool,
            _ => ExecutionMessageSender::System,
        }
    }
}
