use common::error::AppError;
use models::{ConversationContent, ConversationMessageType, ConversationRecord};

use chrono::Utc;

pub struct CreateConversationCommand {
    pub id: i64,
    pub thread_id: Option<i64>,
    pub board_item_id: Option<i64>,
    pub execution_run_id: Option<i64>,
    pub content: ConversationContent,
    pub message_type: ConversationMessageType,
    pub metadata: Option<serde_json::Value>,
}

impl CreateConversationCommand {
    pub fn new(
        id: i64,
        thread_id: i64,
        content: ConversationContent,
        message_type: ConversationMessageType,
    ) -> Self {
        Self {
            id,
            thread_id: Some(thread_id),
            board_item_id: None,
            execution_run_id: None,
            content,
            message_type,
            metadata: None,
        }
    }

    pub fn with_execution_run_id(mut self, execution_run_id: i64) -> Self {
        self.execution_run_id = Some(execution_run_id);
        self
    }

    pub fn with_board_item_id(mut self, board_item_id: i64) -> Self {
        self.board_item_id = Some(board_item_id);
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

impl CreateConversationCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ConversationRecord, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = Utc::now();

        // Convert typed content to JSON for database storage
        let content_json = serde_json::to_value(&self.content)
            .map_err(|e| AppError::Internal(format!("Failed to serialize content: {}", e)))?;

        // Convert enum to string for database storage
        let message_type_str = match self.message_type {
            ConversationMessageType::UserMessage => "user_message",
            ConversationMessageType::Steer => "steer",
            ConversationMessageType::ToolResult => "tool_result",
            ConversationMessageType::SystemDecision => "system_decision",
            ConversationMessageType::ApprovalRequest => "approval_request",
            ConversationMessageType::ApprovalResponse => "approval_response",
            ConversationMessageType::ExecutionSummary => "execution_summary",
            ConversationMessageType::ClarificationRequest => "clarification_request",
            ConversationMessageType::ClarificationResponse => "clarification_response",
            ConversationMessageType::TaskSubscriptionNotification => {
                "task_subscription_notification"
            }
            ConversationMessageType::AssignmentExecutionTrigger => "assignment_execution_trigger",
            ConversationMessageType::TaskRoutingTrigger => "task_routing_trigger",
        };

        let record = sqlx::query_as::<_, ConversationRecord>(
            r#"
            INSERT INTO conversations (
                id, thread_id, board_item_id, execution_run_id, timestamp, content, message_type,
                created_at, updated_at, metadata
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $8, $9
            )
            RETURNING *
            "#,
        )
        .bind(self.id)
        .bind(self.thread_id)
        .bind(self.board_item_id)
        .bind(self.execution_run_id)
        .bind(now)
        .bind(content_json)
        .bind(message_type_str)
        .bind(now)
        .bind(&self.metadata)
        .fetch_one(executor)
        .await
        .map_err(AppError::from)?;

        Ok(record)
    }
}
