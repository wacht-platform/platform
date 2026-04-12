use chrono::{DateTime, Duration as ChronoDuration, Utc};
use common::{HasDbRouter, HasIdProvider, HasNatsJetStreamProvider, error::AppError};
use models::{ProjectTaskSchedule, ThreadEvent};
use sqlx::Row;

use crate::{CreateProjectTaskBoardItemEventCommand, ReconcileProjectTaskBoardItemCommand};

pub struct CreateProjectTaskScheduleCommand {
    pub id: i64,
    pub template_board_item_id: i64,
    pub schedule_kind: String,
    pub interval_seconds: Option<i64>,
    pub next_run_at: DateTime<Utc>,
}

pub struct UpdateProjectTaskScheduleCommand {
    pub schedule_id: i64,
    pub status: Option<String>,
    pub interval_seconds: Option<Option<i64>>,
    pub next_run_at: Option<DateTime<Utc>>,
}

pub struct MaterializeProjectTaskScheduleCommand {
    pub schedule_id: i64,
}

impl CreateProjectTaskScheduleCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ProjectTaskSchedule, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        validate_schedule_kind(&self.schedule_kind, self.interval_seconds)?;
        let now = Utc::now();

        let schedule = sqlx::query_as!(
            ProjectTaskSchedule,
            r#"
            INSERT INTO project_task_schedules (
                id, template_board_item_id, status, schedule_kind, interval_seconds,
                next_run_at, last_enqueued_at, created_at, updated_at
            ) VALUES (
                $1, $2, 'active', $3, $4,
                $5, NULL, $6, $6
            )
            RETURNING
                id, template_board_item_id, status, schedule_kind, interval_seconds,
                next_run_at, last_enqueued_at, created_at, updated_at
            "#,
            self.id,
            self.template_board_item_id,
            self.schedule_kind,
            self.interval_seconds,
            self.next_run_at,
            now,
        )
        .fetch_one(executor)
        .await?;

        Ok(schedule)
    }
}

impl UpdateProjectTaskScheduleCommand {
    pub fn new(schedule_id: i64) -> Self {
        Self {
            schedule_id,
            status: None,
            interval_seconds: None,
            next_run_at: None,
        }
    }

    pub fn with_status(mut self, status: String) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_interval_seconds(mut self, interval_seconds: Option<i64>) -> Self {
        self.interval_seconds = Some(interval_seconds);
        self
    }

    pub fn with_next_run_at(mut self, next_run_at: DateTime<Utc>) -> Self {
        self.next_run_at = Some(next_run_at);
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ProjectTaskSchedule, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = Utc::now();
        let schedule = sqlx::query_as!(
            ProjectTaskSchedule,
            r#"
            UPDATE project_task_schedules
            SET
                status = COALESCE($2, status),
                interval_seconds = COALESCE($3, interval_seconds),
                next_run_at = COALESCE($4, next_run_at),
                updated_at = $5
            WHERE id = $1
            RETURNING
                id, template_board_item_id, status, schedule_kind, interval_seconds,
                next_run_at, last_enqueued_at, created_at, updated_at
            "#,
            self.schedule_id,
            self.status,
            self.interval_seconds.flatten(),
            self.next_run_at,
            now,
        )
        .fetch_one(executor)
        .await?;

        validate_schedule_kind(&schedule.schedule_kind, schedule.interval_seconds)?;
        Ok(schedule)
    }
}

impl MaterializeProjectTaskScheduleCommand {
    pub fn new(schedule_id: i64) -> Self {
        Self { schedule_id }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Option<ThreadEvent>, AppError>
    where
        D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + ?Sized,
    {
        let mut tx = deps.writer_pool().begin().await?;

        let row = sqlx::query(
            r#"
            SELECT
                s.id,
                s.template_board_item_id,
                s.status,
                s.schedule_kind,
                s.interval_seconds,
                s.next_run_at,
                s.last_enqueued_at,
                i.task_key,
                i.title,
                i.description,
                i.status AS board_item_status,
                i.priority,
                i.metadata,
                b.deployment_id,
                b.project_id,
                p.coordinator_thread_id
            FROM project_task_schedules s
            INNER JOIN project_task_board_items i
                ON i.id = s.template_board_item_id
               AND i.archived_at IS NULL
            INNER JOIN project_task_boards b
                ON b.id = i.board_id
               AND b.archived_at IS NULL
            INNER JOIN actor_projects p
                ON p.id = b.project_id
               AND p.archived_at IS NULL
            WHERE s.id = $1
              AND s.status = 'active'
              AND s.next_run_at <= $2
            FOR UPDATE SKIP LOCKED
            "#,
        )
        .bind(self.schedule_id)
        .bind(Utc::now() + ChronoDuration::minutes(30))
        .fetch_optional(&mut *tx)
        .await?;

        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };

        let now = Utc::now();

        let schedule_id: i64 = row.get("id");
        let template_board_item_id: i64 = row.get("template_board_item_id");
        let schedule_kind: String = row.get("schedule_kind");
        let interval_seconds: Option<i64> = row.get("interval_seconds");
        let next_run_at_raw: chrono::DateTime<Utc> = row.get("next_run_at");
        let coordinator_thread_id: Option<i64> = row.get("coordinator_thread_id");

        CreateProjectTaskBoardItemEventCommand {
            id: deps.id_provider().next_id()? as i64,
            board_item_id: template_board_item_id,
            thread_id: coordinator_thread_id,
            execution_run_id: None,
            event_type: "task_scheduled".to_string(),
            summary: "Scheduled task run queued".to_string(),
            body_markdown: None,
            details: serde_json::json!({
                "schedule_id": schedule_id.to_string(),
                "template_board_item_id": template_board_item_id.to_string(),
                "scheduled_for": next_run_at_raw,
                "schedule_kind": schedule_kind,
            }),
        }
        .execute_with_db(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE project_task_board_items
            SET
                status = 'pending',
                assigned_thread_id = NULL,
                completed_at = NULL,
                updated_at = $2
            WHERE id = $1
            "#,
        )
        .bind(template_board_item_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let next_run_at =
            next_schedule_run_at(&schedule_kind, interval_seconds, next_run_at_raw, now)?;

        let next_status = if next_run_at.is_some() {
            models::project_task_schedule::status::ACTIVE
        } else {
            models::project_task_schedule::status::COMPLETED
        };

        sqlx::query!(
            r#"
            UPDATE project_task_schedules
            SET
                status = $2,
                next_run_at = COALESCE($3, next_run_at),
                last_enqueued_at = $4,
                updated_at = $4
            WHERE id = $1
            "#,
            schedule_id,
            next_status,
            next_run_at,
            now,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        ReconcileProjectTaskBoardItemCommand::new(template_board_item_id)
            .with_note(
                "Scheduled task became due; reset task to pending and reevaluated routing"
                    .to_string(),
            )
            .execute_with_deps(deps)
            .await?;
        Ok(None)
    }
}

fn validate_schedule_kind(
    schedule_kind: &str,
    interval_seconds: Option<i64>,
) -> Result<(), AppError> {
    match schedule_kind {
        models::project_task_schedule::schedule_kind::ONCE => {
            if interval_seconds.is_some() {
                return Err(AppError::BadRequest(
                    "once schedules must not set interval_seconds".to_string(),
                ));
            }
        }
        models::project_task_schedule::schedule_kind::INTERVAL => {
            if interval_seconds.unwrap_or(0) <= 0 {
                return Err(AppError::BadRequest(
                    "interval schedules must set interval_seconds > 0".to_string(),
                ));
            }
        }
        other => {
            return Err(AppError::BadRequest(format!(
                "Unsupported schedule_kind '{}'",
                other
            )));
        }
    }

    Ok(())
}

fn next_schedule_run_at(
    schedule_kind: &str,
    interval_seconds: Option<i64>,
    current_next_run_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, AppError> {
    match schedule_kind {
        models::project_task_schedule::schedule_kind::ONCE => Ok(None),
        models::project_task_schedule::schedule_kind::INTERVAL => {
            let seconds = interval_seconds.ok_or_else(|| {
                AppError::BadRequest("interval schedule is missing interval_seconds".to_string())
            })?;
            let mut next_run_at = current_next_run_at;
            while next_run_at <= now {
                next_run_at += ChronoDuration::seconds(seconds);
            }
            Ok(Some(next_run_at))
        }
        other => Err(AppError::BadRequest(format!(
            "Unsupported schedule_kind '{}'",
            other
        ))),
    }
}
