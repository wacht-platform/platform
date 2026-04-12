use common::error::AppError;
use models::ThreadEvent;

pub struct GetThreadEventByIdQuery {
    pub event_id: i64,
}

impl GetThreadEventByIdQuery {
    pub fn new(event_id: i64) -> Self {
        Self { event_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<ThreadEvent>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let event = sqlx::query_as!(
            ThreadEvent,
            r#"
            SELECT
                id, deployment_id, thread_id, board_item_id, event_type, status,
                priority, payload, available_at, claimed_at, completed_at, failed_at,
                caused_by_conversation_id, caused_by_run_id, caused_by_thread_id, created_at, updated_at
            FROM thread_events
            WHERE id = $1
            "#,
            self.event_id
        )
        .fetch_optional(executor)
        .await?;

        Ok(event)
    }
}

pub struct ListPendingThreadEventsQuery {
    pub thread_id: i64,
}

impl ListPendingThreadEventsQuery {
    pub fn new(thread_id: i64) -> Self {
        Self { thread_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<ThreadEvent>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let events = sqlx::query_as!(
            ThreadEvent,
            r#"
            SELECT
                id, deployment_id, thread_id, board_item_id, event_type, status,
                priority, payload, available_at, claimed_at, completed_at, failed_at,
                caused_by_conversation_id, caused_by_run_id, caused_by_thread_id, created_at, updated_at
            FROM thread_events
            WHERE thread_id = $1 AND status = $2 AND available_at <= NOW()
            ORDER BY priority ASC, available_at ASC, created_at ASC
            "#,
            self.thread_id,
            models::thread_event::status::PENDING,
        )
        .fetch_all(executor)
        .await?;

        Ok(events)
    }
}

pub struct ListThreadsWithDuePendingThreadEventsQuery {
    pub limit: i64,
}

impl ListThreadsWithDuePendingThreadEventsQuery {
    pub fn new(limit: i64) -> Self {
        Self { limit }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<(i64, i64)>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT deployment_id, thread_id
            FROM thread_events
            WHERE status = $1
              AND available_at <= NOW()
              AND event_type IN ($2, $3, $4)
              AND NOT EXISTS (
                    SELECT 1
                    FROM thread_events claimed
                    WHERE claimed.thread_id = thread_events.thread_id
                      AND claimed.status = $5
              )
            ORDER BY deployment_id ASC, thread_id ASC
            LIMIT $6
            "#,
            models::thread_event::status::PENDING,
            models::thread_event::event_type::TASK_ROUTING,
            models::thread_event::event_type::ASSIGNMENT_EXECUTION,
            models::thread_event::event_type::ASSIGNMENT_OUTCOME_REVIEW,
            models::thread_event::status::CLAIMED,
            self.limit,
        )
        .fetch_all(executor)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| (row.deployment_id, row.thread_id))
            .collect())
    }
}
