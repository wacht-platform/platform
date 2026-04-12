use common::{HasNatsProvider, error::AppError};
use dto::json::NatsTaskMessage;
use models::{ConversationContent, ConversationMessageType, ConversationRecord};

use chrono::Utc;

pub struct CreateConversationCommand {
    pub id: i64,
    pub thread_id: i64,
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
            thread_id,
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

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

pub struct CleanupCompactedConversationsCommand {
    pub thread_id: i64,
    pub cleanup_through_id: i64,
}

impl CleanupCompactedConversationsCommand {
    pub fn new(thread_id: i64, cleanup_through_id: i64) -> Self {
        Self {
            thread_id,
            cleanup_through_id,
        }
    }
}

impl CleanupCompactedConversationsCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<u64, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query(
            r#"
            DELETE FROM conversations
            WHERE thread_id = $1
              AND id <= $2
              AND message_type <> 'execution_summary'
            "#,
        )
        .bind(self.thread_id)
        .bind(self.cleanup_through_id)
        .execute(executor)
        .await
        .map_err(AppError::from)?;

        Ok(result.rows_affected())
    }
}

pub struct DispatchConversationCleanupTaskCommand {
    pub thread_id: i64,
    pub cleanup_through_id: i64,
}

impl DispatchConversationCleanupTaskCommand {
    pub fn new(thread_id: i64, cleanup_through_id: i64) -> Self {
        Self {
            thread_id,
            cleanup_through_id,
        }
    }
}

impl DispatchConversationCleanupTaskCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasNatsProvider + ?Sized,
    {
        let task_message = NatsTaskMessage {
            task_type: "conversation.cleanup_compacted".to_string(),
            task_id: format!(
                "conversation-cleanup-{}-{}",
                self.thread_id, self.cleanup_through_id
            ),
            payload: serde_json::json!({
                "thread_id": self.thread_id,
                "cleanup_through_id": self.cleanup_through_id
            }),
        };

        deps.nats_provider()
            .publish(
                "worker.tasks.conversation.cleanup_compacted",
                serde_json::to_vec(&task_message)
                    .map_err(|e| AppError::Internal(format!("Failed to serialize task: {}", e)))?
                    .into(),
            )
            .await
            .map_err(|e| {
                AppError::Internal(format!(
                    "Failed to publish conversation cleanup task to NATS: {}",
                    e
                ))
            })?;

        Ok(())
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
        };

        let record = sqlx::query_as::<_, ConversationRecord>(
            r#"
            INSERT INTO conversations (
                id, thread_id, execution_run_id, timestamp, content, message_type,
                created_at, updated_at, metadata
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $7, $8
            )
            RETURNING *
            "#,
        )
        .bind(self.id)
        .bind(self.thread_id)
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
