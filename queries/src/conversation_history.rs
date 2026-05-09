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
                id, thread_id, board_item_id, execution_run_id, timestamp, content, message_type,
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
        "clarification_request" => Ok(ConversationMessageType::ClarificationRequest),
        "clarification_response" => Ok(ConversationMessageType::ClarificationResponse),
        "task_subscription_notification" => Ok(ConversationMessageType::TaskSubscriptionNotification),
        other => Err(AppError::Internal(format!(
            "Unknown conversation message_type '{}'",
            other
        ))),
    }
}

fn build_conversation_record(
    id: i64,
    thread_id: Option<i64>,
    board_item_id: Option<i64>,
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
        board_item_id,
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
                id, thread_id, board_item_id, execution_run_id, timestamp, content, message_type,
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
            record.board_item_id,
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
    pub board_item_id: Option<i64>,
}

impl GetLLMConversationHistoryQuery {
    pub fn new(thread_id: i64) -> Self {
        Self {
            thread_id,
            board_item_id: None,
        }
    }

    pub fn with_board_item_id(mut self, board_item_id: Option<i64>) -> Self {
        self.board_item_id = board_item_id;
        self
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
                  AND ($2::bigint IS NULL OR board_item_id = $2)
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
                SELECT c.id, c.thread_id, c.board_item_id, c.execution_run_id, c.timestamp, c.content, c.message_type,
                       c.created_at, c.updated_at, c.metadata
                FROM conversations c, last_summary_with_default ls
                WHERE c.thread_id = $1
                  AND ($2::bigint IS NULL OR c.board_item_id = $2)
                  AND c.id > ls.last_summary_id
            ),
            execution_summaries AS (
                SELECT c.*,
                       ROW_NUMBER() OVER (ORDER BY c.id DESC) as execution_count
                FROM conversations c
                WHERE c.thread_id = $1
                  AND ($2::bigint IS NULL OR c.board_item_id = $2)
                  AND c.message_type = 'execution_summary'
                ORDER BY c.id DESC
            ),
            limited_summaries AS (
                SELECT id, thread_id, board_item_id, execution_run_id, timestamp, content, message_type,
                       created_at, updated_at, metadata
                FROM execution_summaries
                WHERE execution_count <= 3
            )
            SELECT * FROM recent_unsummarized
            UNION ALL
            SELECT * FROM limited_summaries
            ORDER BY id ASC
            "#,
            self.thread_id,
            self.board_item_id,
        )
        .fetch_all(executor)
        .await
        .map_err(AppError::from)?;

        records
            .into_iter()
            .map(|row| {
                build_conversation_record(
                    require_conversation_field(row.id, "id")?,
                    row.thread_id,
                    row.board_item_id,
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
    pub board_item_id: Option<i64>,
}

impl GetCompactionWindowConversationsQuery {
    pub fn with_board_item_id(mut self, board_item_id: Option<i64>) -> Self {
        self.board_item_id = board_item_id;
        self
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
                  AND ($3::bigint IS NULL OR board_item_id = $3)
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
                c.id, c.thread_id, c.board_item_id, c.execution_run_id, c.timestamp, c.content, c.message_type,
                c.created_at, c.updated_at, c.metadata
            FROM conversations c, last_summary_with_default ls
            WHERE (
                ($3::bigint IS NOT NULL AND c.board_item_id = $3)
                OR ($3::bigint IS NULL AND c.thread_id = $1)
            )
              AND c.id > ls.last_summary_id
              AND c.id < $2
              AND c.message_type <> 'execution_summary'
            ORDER BY c.id ASC
            "#,
            self.thread_id,
            self.before_conversation_id,
            self.board_item_id,
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
                    row.board_item_id,
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

#[derive(Debug)]
pub struct ListThreadMessagesForUserQuery {
    pub thread_id: i64,
    pub limit: i64,
    pub before_id: Option<i64>,
    pub after_id: Option<i64>,
}

impl ListThreadMessagesForUserQuery {
    pub fn new(thread_id: i64, limit: i64) -> Self {
        Self {
            thread_id,
            limit,
            before_id: None,
            after_id: None,
        }
    }

    pub fn with_before_id(mut self, before_id: Option<i64>) -> Self {
        self.before_id = before_id;
        self
    }

    pub fn with_after_id(mut self, after_id: Option<i64>) -> Self {
        self.after_id = after_id;
        self
    }

    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Vec<ConversationRecord>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            r#"
            SELECT id, thread_id, board_item_id, execution_run_id, timestamp,
                   content, message_type, created_at, updated_at, metadata
            FROM conversations
            WHERE thread_id = $1
              AND message_type <> 'task_subscription_notification'
              AND ($2::bigint IS NULL OR id < $2)
              AND ($3::bigint IS NULL OR id > $3)
            ORDER BY
              CASE WHEN $3::bigint IS NOT NULL THEN id END ASC,
              CASE WHEN $3::bigint IS NULL THEN id END DESC
            LIMIT $4
            "#,
            self.thread_id,
            self.before_id,
            self.after_id,
            self.limit,
        )
        .fetch_all(executor)
        .await
        .map_err(AppError::from)?;

        rows.into_iter()
            .map(|row| {
                Ok(ConversationRecord {
                    id: row.id,
                    thread_id: row.thread_id,
                    board_item_id: row.board_item_id,
                    execution_run_id: row.execution_run_id,
                    timestamp: row.timestamp,
                    content: serde_json::from_value(row.content).map_err(|e| {
                        AppError::Internal(format!(
                            "Failed to deserialize conversation content: {}",
                            e
                        ))
                    })?,
                    message_type: parse_conversation_message_type(&row.message_type)?,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                    metadata: row.metadata,
                })
            })
            .collect()
    }
}

pub struct ListConversationsForBoardItemQuery {
    pub board_item_id: i64,
    pub limit: i64,
}

impl ListConversationsForBoardItemQuery {
    pub fn new(board_item_id: i64, limit: i64) -> Self {
        Self {
            board_item_id,
            limit,
        }
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
                id, thread_id, board_item_id, execution_run_id, timestamp, content, message_type,
                created_at, updated_at, metadata
            FROM conversations
            WHERE board_item_id = $1
                AND message_type != 'execution_summary'
            ORDER BY id DESC
            LIMIT $2
            "#,
        )
        .bind(self.board_item_id)
        .bind(self.limit)
        .fetch_all(executor)
        .await
        .map_err(AppError::from)?;

        Ok(records)
    }
}

#[derive(Debug, Clone)]
pub struct TaskRoutingEventRecord {
    pub id: i64,
    pub coordinator_thread_id: Option<i64>,
    pub routing_reason: Option<String>,
    pub summary: Option<String>,
    pub note: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Debug)]
pub struct GetBoardItemConversationHistoryQuery {
    pub board_item_id: i64,
    pub own_thread_id: i64,
}

impl GetBoardItemConversationHistoryQuery {
    pub fn new(board_item_id: i64, own_thread_id: i64) -> Self {
        Self {
            board_item_id,
            own_thread_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ConversationRecord>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query_as::<_, ConversationRecord>(
            r#"
            WITH last_summary AS (
                SELECT id AS last_summary_id
                FROM conversations
                WHERE thread_id = $2
                  AND board_item_id = $1
                  AND message_type = 'execution_summary'
                ORDER BY id DESC
                LIMIT 1
            ),
            last_summary_with_default AS (
                SELECT COALESCE(last_summary_id, 0) AS last_summary_id
                FROM (SELECT 1) dummy
                LEFT JOIN last_summary ON TRUE
            )
            SELECT
                c.id, c.thread_id, c.board_item_id, c.execution_run_id, c.timestamp, c.content, c.message_type,
                c.created_at, c.updated_at, c.metadata
            FROM conversations c, last_summary_with_default ls
            WHERE c.board_item_id = $1
              AND c.id >= ls.last_summary_id
            ORDER BY c.id ASC
            "#,
        )
        .bind(self.board_item_id)
        .bind(self.own_thread_id)
        .fetch_all(executor)
        .await
        .map_err(AppError::from)
    }
}

#[derive(Debug, Clone)]
pub struct TaskThreadMetaRecord {
    pub thread_id: i64,
    pub title: String,
    pub thread_purpose: String,
}

#[derive(Debug)]
pub struct GetBoardItemThreadMetaQuery {
    pub board_item_id: i64,
}

impl GetBoardItemThreadMetaQuery {
    pub fn new(board_item_id: i64) -> Self {
        Self { board_item_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<TaskThreadMetaRecord>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT
                t.id            AS "id!",
                t.title         AS "title!",
                t.thread_purpose AS "thread_purpose!"
            FROM agent_threads t
            INNER JOIN conversations c ON c.thread_id = t.id
            WHERE c.board_item_id = $1
            "#,
            self.board_item_id,
        )
        .fetch_all(executor)
        .await
        .map_err(AppError::from)?;
        Ok(rows
            .into_iter()
            .map(|r| TaskThreadMetaRecord {
                thread_id: r.id,
                title: r.title,
                thread_purpose: r.thread_purpose,
            })
            .collect())
    }
}

#[derive(Debug)]
pub struct GetBoardItemRoutingEventsQuery {
    pub board_item_id: i64,
    pub limit: i64,
}

impl GetBoardItemRoutingEventsQuery {
    pub fn new(board_item_id: i64) -> Self {
        Self {
            board_item_id,
            limit: 100,
        }
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.limit = limit;
        self
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<TaskRoutingEventRecord>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            r#"
            SELECT id           AS "id!",
                   payload      AS "payload!",
                   created_at   AS "created_at!"
            FROM event_log
            WHERE aggregate_type = 'board_item'
              AND aggregate_id = $1
              AND event_type = 'task_routing'
            ORDER BY created_at ASC
            LIMIT $2
            "#,
            self.board_item_id,
            self.limit,
        )
        .fetch_all(executor)
        .await
        .map_err(AppError::from)?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let payload: serde_json::Value = r.payload;
                let str_field = |key: &str| -> Option<String> {
                    payload
                        .get(key)
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                };
                let coordinator_thread_id =
                    str_field("thread_id").and_then(|s| s.parse::<i64>().ok());
                TaskRoutingEventRecord {
                    id: r.id,
                    coordinator_thread_id,
                    routing_reason: str_field("routing_reason"),
                    summary: str_field("summary"),
                    note: str_field("note"),
                    created_at: r.created_at,
                }
            })
            .collect())
    }
}
