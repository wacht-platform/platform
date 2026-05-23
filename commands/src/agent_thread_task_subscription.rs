use common::ResultExt;
use common::error::AppError;
use models::{AgentThreadTaskSubscription, TaskSubscriptionEventKind};
use sqlx::Postgres;

pub struct UpsertAgentThreadTaskSubscriptionCommand {
    pub deployment_id: i64,
    pub thread_id: i64,
    pub board_item_id: i64,
    pub event_kinds: Vec<TaskSubscriptionEventKind>,
}

impl UpsertAgentThreadTaskSubscriptionCommand {
    pub async fn execute<'e, E>(self, executor: E) -> Result<AgentThreadTaskSubscription, AppError>
    where
        E: sqlx::Executor<'e, Database = Postgres>,
    {
        let kinds_json =
            serde_json::to_value(&self.event_kinds).map_err_internal("serialize event_kinds")?;

        let row = sqlx::query!(
            r#"
            INSERT INTO agent_thread_task_subscriptions
                (deployment_id, thread_id, board_item_id, event_kinds)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (thread_id, board_item_id) DO UPDATE
                SET event_kinds = EXCLUDED.event_kinds,
                    updated_at = NOW()
            RETURNING deployment_id, thread_id, board_item_id, event_kinds,
                      created_at, updated_at
            "#,
            self.deployment_id,
            self.thread_id,
            self.board_item_id,
            kinds_json,
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(AgentThreadTaskSubscription {
            deployment_id: row.deployment_id,
            thread_id: row.thread_id,
            board_item_id: row.board_item_id,
            event_kinds: serde_json::from_value(row.event_kinds).unwrap_or_default(),
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

pub struct DeleteAgentThreadTaskSubscriptionCommand {
    pub thread_id: i64,
    pub board_item_id: i64,
}

impl DeleteAgentThreadTaskSubscriptionCommand {
    pub async fn execute<'e, E>(self, executor: E) -> Result<bool, AppError>
    where
        E: sqlx::Executor<'e, Database = Postgres>,
    {
        let res = sqlx::query!(
            r#"
            DELETE FROM agent_thread_task_subscriptions
            WHERE thread_id = $1 AND board_item_id = $2
            "#,
            self.thread_id,
            self.board_item_id,
        )
        .execute(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(res.rows_affected() > 0)
    }
}

pub struct DeleteSubscriptionsForThreadCommand {
    pub thread_id: i64,
}

impl DeleteSubscriptionsForThreadCommand {
    pub async fn execute<'e, E>(self, executor: E) -> Result<u64, AppError>
    where
        E: sqlx::Executor<'e, Database = Postgres>,
    {
        let res = sqlx::query!(
            r#"DELETE FROM agent_thread_task_subscriptions WHERE thread_id = $1"#,
            self.thread_id,
        )
        .execute(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(res.rows_affected())
    }
}

pub struct DeleteSubscriptionsForBoardItemCommand {
    pub board_item_id: i64,
}

impl DeleteSubscriptionsForBoardItemCommand {
    pub async fn execute<'e, E>(self, executor: E) -> Result<u64, AppError>
    where
        E: sqlx::Executor<'e, Database = Postgres>,
    {
        let res = sqlx::query!(
            r#"DELETE FROM agent_thread_task_subscriptions WHERE board_item_id = $1"#,
            self.board_item_id,
        )
        .execute(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(res.rows_affected())
    }
}
