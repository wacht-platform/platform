use common::error::AppError;
use models::AgentThreadTaskSubscription;

pub struct GetAgentThreadTaskSubscriptionQuery {
    pub thread_id: i64,
    pub board_item_id: i64,
}

impl GetAgentThreadTaskSubscriptionQuery {
    pub fn new(thread_id: i64, board_item_id: i64) -> Self {
        Self {
            thread_id,
            board_item_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Option<AgentThreadTaskSubscription>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT deployment_id, thread_id, board_item_id, event_kinds,
                   created_at, updated_at
            FROM agent_thread_task_subscriptions
            WHERE thread_id = $1 AND board_item_id = $2
            "#,
            self.thread_id,
            self.board_item_id,
        )
        .fetch_optional(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(row.map(|r| AgentThreadTaskSubscription {
            deployment_id: r.deployment_id,
            thread_id: r.thread_id,
            board_item_id: r.board_item_id,
            event_kinds: serde_json::from_value(r.event_kinds.clone()).unwrap_or_else(|e| {
                tracing::warn!(
                    deployment_id = r.deployment_id,
                    thread_id = r.thread_id,
                    board_item_id = r.board_item_id,
                    error = %e,
                    raw = %r.event_kinds,
                    "agent_thread_task_subscriptions.event_kinds: deserialize failed; falling back to empty"
                );
                Default::default()
            }),
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }
}

pub struct ListSubscribersForBoardItemQuery {
    pub board_item_id: i64,
    pub event_kind: String,
}

impl ListSubscribersForBoardItemQuery {
    pub fn new(board_item_id: i64, event_kind: impl Into<String>) -> Self {
        Self {
            board_item_id,
            event_kind: event_kind.into(),
        }
    }

    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Vec<AgentThreadTaskSubscription>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            r#"
            SELECT deployment_id, thread_id, board_item_id, event_kinds,
                   created_at, updated_at
            FROM agent_thread_task_subscriptions
            WHERE board_item_id = $1
              AND event_kinds @> to_jsonb($2::text)
            "#,
            self.board_item_id,
            self.event_kind,
        )
        .fetch_all(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(rows
            .into_iter()
            .map(|r| AgentThreadTaskSubscription {
                deployment_id: r.deployment_id,
                thread_id: r.thread_id,
                board_item_id: r.board_item_id,
                event_kinds: serde_json::from_value(r.event_kinds.clone()).unwrap_or_else(|e| {
                tracing::warn!(
                    deployment_id = r.deployment_id,
                    thread_id = r.thread_id,
                    board_item_id = r.board_item_id,
                    error = %e,
                    raw = %r.event_kinds,
                    "agent_thread_task_subscriptions.event_kinds: deserialize failed; falling back to empty"
                );
                Default::default()
            }),
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }
}

pub struct PendingSubscriptionNotification {
    pub id: i64,
    pub task_key: String,
    pub task_title: String,
    pub from_status: String,
    pub to_status: String,
    pub transitioned_at: String,
}

pub struct ListPendingSubscriptionNotificationsQuery {
    pub thread_id: i64,
}

impl ListPendingSubscriptionNotificationsQuery {
    pub fn new(thread_id: i64) -> Self {
        Self { thread_id }
    }

    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Vec<PendingSubscriptionNotification>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            r#"
            SELECT id, content
            FROM conversations
            WHERE thread_id = $1
              AND message_type = 'task_subscription_notification'
              AND COALESCE(metadata->>'consumed_at', '') = ''
            ORDER BY created_at ASC
            "#,
            self.thread_id,
        )
        .fetch_all(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let c = r.content;
                PendingSubscriptionNotification {
                    id: r.id,
                    task_key: c
                        .get("task_key")
                        .and_then(|v| v.as_str())
                        .unwrap_or("UNKNOWN")
                        .to_string(),
                    task_title: c
                        .get("task_title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(untitled)")
                        .to_string(),
                    from_status: c
                        .get("from_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?")
                        .to_string(),
                    to_status: c
                        .get("to_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?")
                        .to_string(),
                    transitioned_at: c
                        .get("transitioned_at")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }
            })
            .collect())
    }
}

pub struct ListThreadTaskSubscriptionsQuery {
    pub thread_id: i64,
}

impl ListThreadTaskSubscriptionsQuery {
    pub fn new(thread_id: i64) -> Self {
        Self { thread_id }
    }

    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Vec<AgentThreadTaskSubscription>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            r#"
            SELECT deployment_id, thread_id, board_item_id, event_kinds,
                   created_at, updated_at
            FROM agent_thread_task_subscriptions
            WHERE thread_id = $1
            ORDER BY created_at DESC
            "#,
            self.thread_id,
        )
        .fetch_all(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(rows
            .into_iter()
            .map(|r| AgentThreadTaskSubscription {
                deployment_id: r.deployment_id,
                thread_id: r.thread_id,
                board_item_id: r.board_item_id,
                event_kinds: serde_json::from_value(r.event_kinds.clone()).unwrap_or_else(|e| {
                tracing::warn!(
                    deployment_id = r.deployment_id,
                    thread_id = r.thread_id,
                    board_item_id = r.board_item_id,
                    error = %e,
                    raw = %r.event_kinds,
                    "agent_thread_task_subscriptions.event_kinds: deserialize failed; falling back to empty"
                );
                Default::default()
            }),
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }
}
