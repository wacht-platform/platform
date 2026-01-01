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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ConversationMessageType {
    UserMessage,
    AgentResponse,
    AssistantAcknowledgment,
    ActionExecutionResult,
    SystemDecision,
    ContextResults,
    UserInputRequest,
    ExecutionSummary,
    PlatformFunctionResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConversationContent {
    UserMessage {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        images: Option<Vec<ImageData>>,
    },
    AgentResponse {
        response: String,
        context_used: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        thought_signature: Option<String>,
    },
    AssistantAcknowledgment {
        acknowledgment_message: String,
        further_action_required: bool,
        reasoning: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thought_signature: Option<String>,
    },
    ActionExecutionResult {
        task_execution: Value,
        execution_status: String,
        blocking_reason: Option<String>,
    },
    SystemDecision {
        step: String,
        reasoning: String,
        confidence: f32,
        #[serde(skip_serializing_if = "Option::is_none")]
        thought_signature: Option<String>,
    },
    ContextResults {
        query: String,
        results: Value,
        result_count: usize,
        timestamp: DateTime<Utc>,
    },
    UserInputRequest {
        question: String,
        context: String,
        input_type: String,
        options: Option<Vec<String>>,
        default_value: Option<String>,
        placeholder: Option<String>,
    },
    ExecutionSummary {
        user_message: String,
        agent_execution: String,
        token_count: usize,
    },
    PlatformFunctionResult {
        execution_id: String,
        result: String,
    },
}

/// Conversation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRecord {
    pub id: i64,
    pub context_id: i64,
    pub timestamp: DateTime<Utc>,
    pub content: ConversationContent,
    pub message_type: ConversationMessageType,
    pub token_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl sqlx::FromRow<'_, sqlx::postgres::PgRow> for ConversationRecord {
    fn from_row(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        let message_type_str: String = row.try_get("message_type")?;
        let message_type = match message_type_str.as_str() {
            "user_message" => ConversationMessageType::UserMessage,
            "agent_response" => ConversationMessageType::AgentResponse,
            "assistant_acknowledgment" => ConversationMessageType::AssistantAcknowledgment,
            "action_execution_result" => ConversationMessageType::ActionExecutionResult,
            "system_decision" => ConversationMessageType::SystemDecision,
            "context_results" => ConversationMessageType::ContextResults,
            "user_input_request" => ConversationMessageType::UserInputRequest,
            "execution_summary" => ConversationMessageType::ExecutionSummary,
            "platform_function_result" => ConversationMessageType::PlatformFunctionResult,
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
            context_id: row.try_get("context_id")?,
            timestamp: row.try_get("timestamp")?,
            content,
            message_type,
            token_count: row.try_get("token_count").unwrap_or(0),
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}
