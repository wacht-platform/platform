use common::error::AppError;
use models::TaskHandoffSummary;

#[derive(Debug)]
pub struct GetRecentTaskHandoffSummariesQuery {
    pub board_item_id: i64,
    pub limit: i64,
}

impl GetRecentTaskHandoffSummariesQuery {
    pub fn new(board_item_id: i64, limit: i64) -> Self {
        Self {
            board_item_id,
            limit,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<TaskHandoffSummary>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query_as!(
            TaskHandoffSummary,
            r#"
            SELECT
                id, deployment_id, board_item_id, thread_id,
                assignment_id, execution_run_id,
                assignment_role, outcome, summary,
                artifacts, blockers, next_actions, metadata,
                created_at, updated_at
            FROM task_handoff_summaries
            WHERE board_item_id = $1
            ORDER BY created_at DESC
            LIMIT $2
            "#,
            self.board_item_id,
            self.limit,
        )
        .fetch_all(executor)
        .await?;
        Ok(rows)
    }
}

#[derive(Debug)]
pub struct GetTaskHandoffSummariesByIdsQuery {
    pub ids: Vec<i64>,
}

impl GetTaskHandoffSummariesByIdsQuery {
    pub fn new(ids: Vec<i64>) -> Self {
        Self { ids }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<TaskHandoffSummary>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if self.ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as!(
            TaskHandoffSummary,
            r#"
            SELECT
                id, deployment_id, board_item_id, thread_id,
                assignment_id, execution_run_id,
                assignment_role, outcome, summary,
                artifacts, blockers, next_actions, metadata,
                created_at, updated_at
            FROM task_handoff_summaries
            WHERE id = ANY($1)
            ORDER BY created_at DESC
            "#,
            &self.ids,
        )
        .fetch_all(executor)
        .await?;
        Ok(rows)
    }
}
