use chrono::{DateTime, Utc};
use common::error::AppError;
use models::ProjectTaskSchedule;

pub struct ListDueProjectTaskScheduleIdsQuery {
    pub due_before: DateTime<Utc>,
    pub limit: i64,
}

pub struct GetProjectTaskScheduleByIdQuery {
    pub schedule_id: i64,
}

pub struct GetProjectTaskScheduleByTaskKeyQuery {
    pub board_id: i64,
    pub task_key: String,
}

impl ListDueProjectTaskScheduleIdsQuery {
    pub fn new(due_before: DateTime<Utc>, limit: i64) -> Self {
        Self { due_before, limit }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            r#"
            SELECT id
            FROM project_task_schedules
            WHERE status = 'active'
              AND next_run_at <= $1
            ORDER BY next_run_at ASC
            LIMIT $2
            "#,
            self.due_before,
            self.limit,
        )
        .fetch_all(executor)
        .await?;

        Ok(rows.into_iter().map(|row| row.id).collect())
    }
}

impl GetProjectTaskScheduleByIdQuery {
    pub fn new(schedule_id: i64) -> Self {
        Self { schedule_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ProjectTaskSchedule>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query_as!(
            ProjectTaskSchedule,
            r#"
            SELECT
                id, board_id, task_key, template_payload, state, state_version,
                status, schedule_kind, interval_seconds, next_run_at, last_fired_at,
                overlap_policy, created_at, updated_at
            FROM project_task_schedules
            WHERE id = $1
            "#,
            self.schedule_id,
        )
        .fetch_optional(executor)
        .await?;

        Ok(row)
    }
}

impl GetProjectTaskScheduleByTaskKeyQuery {
    pub fn new(board_id: i64, task_key: impl Into<String>) -> Self {
        Self {
            board_id,
            task_key: task_key.into(),
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ProjectTaskSchedule>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query_as!(
            ProjectTaskSchedule,
            r#"
            SELECT
                id, board_id, task_key, template_payload, state, state_version,
                status, schedule_kind, interval_seconds, next_run_at, last_fired_at,
                overlap_policy, created_at, updated_at
            FROM project_task_schedules
            WHERE board_id = $1
              AND task_key = $2
            LIMIT 1
            "#,
            self.board_id,
            self.task_key,
        )
        .fetch_optional(executor)
        .await?;

        Ok(row)
    }
}
