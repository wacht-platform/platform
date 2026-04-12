use chrono::Utc;
use common::error::AppError;
use models::{ConversationContent, ConversationMessageType, ConversationRecord};

#[derive(Debug)]
pub struct GetRecentConversationsQuery {
    pub thread_id: i64,
    pub limit: i64,
}

impl GetRecentConversationsQuery {
    pub fn new(thread_id: i64, limit: i64) -> Self {
        Self { thread_id, limit }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ConversationRecord>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let records = sqlx::query_as::<_, ConversationRecord>(
            r#"
            SELECT
                id, thread_id, execution_run_id, timestamp, content, message_type,
                created_at, updated_at, metadata
            FROM conversations
            WHERE thread_id = $1
                AND message_type != 'execution_summary'
            ORDER BY id DESC
            LIMIT $2
            "#,
        )
        .bind(self.thread_id)
        .bind(self.limit)
        .fetch_all(executor)
        .await
        .map_err(AppError::from)?;

        Ok(records)
    }
}

fn parse_conversation_message_type(value: &str) -> Result<ConversationMessageType, AppError> {
    match value {
        "user_message" => Ok(ConversationMessageType::UserMessage),
        "steer" => Ok(ConversationMessageType::Steer),
        "tool_result" => Ok(ConversationMessageType::ToolResult),
        "system_decision" => Ok(ConversationMessageType::SystemDecision),
        "approval_request" => Ok(ConversationMessageType::ApprovalRequest),
        "approval_response" => Ok(ConversationMessageType::ApprovalResponse),
        "execution_summary" => Ok(ConversationMessageType::ExecutionSummary),
        other => Err(AppError::Internal(format!(
            "Unknown conversation message_type '{}'",
            other
        ))),
    }
}

fn build_conversation_record(
    id: i64,
    thread_id: i64,
    execution_run_id: Option<i64>,
    timestamp: chrono::DateTime<Utc>,
    content: serde_json::Value,
    message_type: String,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
    metadata: Option<serde_json::Value>,
) -> Result<ConversationRecord, AppError> {
    Ok(ConversationRecord {
        id,
        thread_id,
        execution_run_id,
        timestamp,
        content: serde_json::from_value::<ConversationContent>(content).map_err(|e| {
            AppError::Internal(format!("Failed to deserialize conversation content: {}", e))
        })?,
        message_type: parse_conversation_message_type(&message_type)?,
        created_at,
        updated_at,
        metadata,
    })
}

fn require_conversation_field<T>(value: Option<T>, field_name: &str) -> Result<T, AppError> {
    value.ok_or_else(|| {
        AppError::Internal(format!(
            "Conversation history query returned NULL for required field '{}'",
            field_name
        ))
    })
}

#[derive(Debug)]
pub struct GetConversationByIdQuery {
    pub conversation_id: i64,
}

impl GetConversationByIdQuery {
    pub fn new(conversation_id: i64) -> Self {
        Self { conversation_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<ConversationRecord, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let record = sqlx::query!(
            r#"
            SELECT
                id, thread_id, execution_run_id, timestamp, content, message_type,
                created_at, updated_at, metadata
            FROM conversations
            WHERE id = $1
            "#,
            self.conversation_id
        )
        .fetch_optional(executor)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| {
            AppError::NotFound(format!("Conversation {} not found", self.conversation_id))
        })?;

        build_conversation_record(
            record.id,
            record.thread_id,
            record.execution_run_id,
            record.timestamp,
            record.content,
            record.message_type,
            record.created_at,
            record.updated_at,
            record.metadata,
        )
    }
}

#[derive(Debug)]
pub struct GetLLMConversationHistoryQuery {
    pub thread_id: i64,
}

impl GetLLMConversationHistoryQuery {
    pub fn new(thread_id: i64) -> Self {
        Self { thread_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ConversationRecord>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let records = sqlx::query!(
            r#"
            WITH last_summary AS (
                SELECT id as last_summary_id
                FROM conversations
                WHERE thread_id = $1
                  AND message_type = 'execution_summary'
                ORDER BY id DESC
                LIMIT 1
            ),
            last_summary_with_default AS (
                SELECT COALESCE(last_summary_id, 0) as last_summary_id
                FROM (SELECT 1) dummy
                LEFT JOIN last_summary ON TRUE
            ),
            recent_unsummarized AS (
                SELECT c.id, c.thread_id, c.execution_run_id, c.timestamp, c.content, c.message_type,
                       c.created_at, c.updated_at, c.metadata
                FROM conversations c, last_summary_with_default ls
                WHERE c.thread_id = $1
                  AND c.id > ls.last_summary_id
            ),
            execution_summaries AS (
                SELECT c.*,
                       ROW_NUMBER() OVER (ORDER BY c.id DESC) as execution_count
                FROM conversations c
                WHERE c.thread_id = $1
                  AND c.message_type = 'execution_summary'
                ORDER BY c.id DESC
            ),
            limited_summaries AS (
                SELECT id, thread_id, execution_run_id, timestamp, content, message_type,
                       created_at, updated_at, metadata
                FROM execution_summaries
                WHERE execution_count <= 3
            )
            SELECT * FROM recent_unsummarized
            UNION ALL
            SELECT * FROM limited_summaries
            ORDER BY id ASC
            "#,
            self.thread_id
        )
        .fetch_all(executor)
        .await
        .map_err(AppError::from)?;

        records
            .into_iter()
            .map(|row| {
                build_conversation_record(
                    require_conversation_field(row.id, "id")?,
                    require_conversation_field(row.thread_id, "thread_id")?,
                    row.execution_run_id,
                    require_conversation_field(row.timestamp, "timestamp")?,
                    require_conversation_field(row.content, "content")?,
                    require_conversation_field(row.message_type, "message_type")?,
                    require_conversation_field(row.created_at, "created_at")?,
                    require_conversation_field(row.updated_at, "updated_at")?,
                    row.metadata,
                )
            })
            .collect()
    }
}

#[derive(Debug)]
pub struct GetCompactionWindowConversationsQuery {
    pub thread_id: i64,
    pub before_conversation_id: i64,
}

impl GetCompactionWindowConversationsQuery {
    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ConversationRecord>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let records = sqlx::query!(
            r#"
            WITH last_summary AS (
                SELECT id as last_summary_id
                FROM conversations
                WHERE thread_id = $1
                  AND message_type = 'execution_summary'
                ORDER BY id DESC
                LIMIT 1
            ),
            last_summary_with_default AS (
                SELECT COALESCE(last_summary_id, 0) as last_summary_id
                FROM (SELECT 1) dummy
                LEFT JOIN last_summary ON TRUE
            )
            SELECT
                c.id, c.thread_id, c.execution_run_id, c.timestamp, c.content, c.message_type,
                c.created_at, c.updated_at, c.metadata
            FROM conversations c, last_summary_with_default ls
            WHERE c.thread_id = $1
              AND c.id > ls.last_summary_id
              AND c.id < $2
              AND c.message_type <> 'execution_summary'
            ORDER BY c.id ASC
            "#,
            self.thread_id,
            self.before_conversation_id
        )
        .fetch_all(executor)
        .await
        .map_err(AppError::from)?;

        records
            .into_iter()
            .map(|row| {
                build_conversation_record(
                    row.id,
                    row.thread_id,
                    row.execution_run_id,
                    row.timestamp,
                    row.content,
                    row.message_type,
                    row.created_at,
                    row.updated_at,
                    row.metadata,
                )
            })
            .collect()
    }
}
