use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ConversationMessageType {
    UserMessage,
    AgentResponse,
    AssistantAcknowledgment,
    AssistantIdeation,
    AssistantActionPlanning,
    AssistantTaskExecution,
    AssistantValidation,
    SystemDecision,
    ContextResults,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConversationContent {
    UserMessage {
        message: String,
    },
    AgentResponse {
        response: String,
        citations: Vec<EnhancedCitation>,
        context_used: Vec<String>,
    },
    AssistantAcknowledgment {
        acknowledgment_message: String,
        further_action_required: bool,
        reasoning: String,
    },
    AssistantIdeation {
        reasoning_summary: String,
        needs_more_iteration: bool,
        context_search_request: Option<String>,
        requires_user_input: bool,
        user_input_request: Option<String>,
        execution_plan: Value,
    },
    AssistantActionPlanning {
        task_execution: Value,
        execution_status: String,
        blocking_reason: Option<String>,
    },
    AssistantTaskExecution {
        task_execution: Value,
        execution_status: String,
        blocking_reason: Option<String>,
    },
    AssistantValidation {
        validation_result: Value,
        loop_decision: String,
        decision_reasoning: String,
        next_iteration_focus: Option<String>,
        has_unresolvable_errors: bool,
        unresolvable_error_details: Option<String>,
    },
    SystemDecision {
        step: String,
        reasoning: String,
        confidence: f32,
    },
    ContextResults {
        query: String,
        results: Value,
        result_count: usize,
        timestamp: DateTime<Utc>,
    },
}

/// Enhanced citation with rich metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedCitation {
    pub item_id: i64,
    pub item_type: CitationType,
    pub relevance_score: f64,
    pub usefulness_score: f64,
    pub confidence: f64,
    pub usage_type: UsageType,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CitationType {
    Memory,
    KnowledgeBase,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageType {
    DirectQuote,
    Paraphrase,
    Inspiration,
    Background,
}

/// Conversation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRecord {
    pub id: i64,
    pub context_id: i64,
    pub timestamp: DateTime<Utc>,
    pub content: ConversationContent,
    pub message_type: ConversationMessageType,
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
            "assistant_ideation" => ConversationMessageType::AssistantIdeation,
            "assistant_action_planning" => ConversationMessageType::AssistantActionPlanning,
            "assistant_task_execution" => ConversationMessageType::AssistantTaskExecution,
            "assistant_validation" => ConversationMessageType::AssistantValidation,
            "system_decision" => ConversationMessageType::SystemDecision,
            "context_results" => ConversationMessageType::ContextResults,
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
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}