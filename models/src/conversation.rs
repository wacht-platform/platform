use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    pub mime_type: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

/// Generic file attachment data (stored)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileData {
    pub filename: String,
    pub mime_type: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationAttachmentType {
    File,
    Folder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationAttachment {
    pub path: String,
    #[serde(rename = "type")]
    pub attachment_type: ConversationAttachmentType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ConversationMessageType {
    UserMessage,
    Steer,
    ToolResult,
    SystemDecision,
    ApprovalRequest,
    ApprovalResponse,
    ExecutionSummary,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    ToolCall,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ExecutionAction {
    #[serde(rename = "type", default = "default_task_type")]
    pub action_type: TaskType,
    pub details: Value,
    #[serde(default = "default_context_messages")]
    pub context_messages: u32,
}

fn default_context_messages() -> u32 {
    1
}

fn default_task_type() -> TaskType {
    TaskType::ToolCall
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ActionsList {
    #[serde(rename = "action")]
    pub actions: Vec<ExecutionAction>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ActionResult {
    pub action: String,
    pub status: ActionResultStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ActionResultStatus {
    Success,
    Error,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TaskExecution {
    pub approach: String,
    pub actions: ActionsList,
    pub expected_result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_result: Option<Vec<ActionResult>>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ActionExecutionStatus {
    Pending,
    Completed,
    Failed,
}

impl Default for ActionExecutionStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConversationContent {
    UserMessage {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        sender_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        files: Option<Vec<FileData>>,
    },
    Steer {
        message: String,
        further_actions_required: bool,
        reasoning: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        attachments: Option<Vec<ConversationAttachment>>,
    },
    ToolResult {
        tool_name: String,
        status: String,
        input: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    SystemDecision {
        step: String,
        reasoning: String,
        confidence: f32,
    },
    ApprovalRequest {
        description: String,
        tools: Vec<RequestedToolApproval>,
    },
    ApprovalResponse {
        #[serde(
            default,
            with = "crate::utils::serde::i64_as_string_option",
            skip_serializing_if = "Option::is_none"
        )]
        request_message_id: Option<i64>,
        approvals: Vec<ToolApprovalDecision>,
    },
    ExecutionSummary {
        user_message: String,
        agent_execution: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestedToolApproval {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub tool_id: i64,
    pub tool_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolApprovalDecision {
    pub tool_name: String,
    pub mode: ToolApprovalMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolApprovalMode {
    AllowOnce,
    AllowAlways,
}

/// Conversation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRecord {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub thread_id: i64,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub execution_run_id: Option<i64>,
    pub timestamp: DateTime<Utc>,
    pub content: ConversationContent,
    pub message_type: ConversationMessageType,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl sqlx::FromRow<'_, sqlx::postgres::PgRow> for ConversationRecord {
    fn from_row(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        let message_type_str: String = row.try_get("message_type")?;
        let message_type = match message_type_str.as_str() {
            "user_message" => ConversationMessageType::UserMessage,
            "steer" => ConversationMessageType::Steer,
            "tool_result" => ConversationMessageType::ToolResult,
            "system_decision" => ConversationMessageType::SystemDecision,
            "approval_request" => ConversationMessageType::ApprovalRequest,
            "approval_response" => ConversationMessageType::ApprovalResponse,
            "execution_summary" => ConversationMessageType::ExecutionSummary,
            _ => {
                return Err(sqlx::Error::ColumnDecode {
                    index: "message_type".to_string(),
                    source: format!("Unknown message type: {}", message_type_str).into(),
                });
            }
        };

        let content_json: Value = row.try_get("content")?;
        let content =
            serde_json::from_value(content_json).map_err(|e| sqlx::Error::ColumnDecode {
                index: "content".to_string(),
                source: e.into(),
            })?;

        Ok(ConversationRecord {
            id: row.try_get("id")?,
            thread_id: row.try_get("thread_id")?,
            execution_run_id: row.try_get("execution_run_id")?,
            timestamp: row.try_get("timestamp")?,
            content,
            message_type,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            metadata: row.try_get("metadata").ok(),
        })
    }
}
