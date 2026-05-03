use chrono::{DateTime, Duration as ChronoDuration, Utc};
use common::{HasDbRouter, HasIdProvider, HasNatsJetStreamProvider, error::AppError};
use models::{
    ProjectTaskBoardItemMetadata, ProjectTaskSchedule, ScheduleCarryover, ScheduleTemplatePayload,
};

use crate::ReconcileProjectTaskBoardItemCommand;

pub struct CreateProjectTaskScheduleCommand {
    pub id: i64,
    pub board_id: i64,
    pub task_key: String,
    pub template_payload: ScheduleTemplatePayload,
    pub schedule_kind: String,
    pub interval_seconds: Option<i64>,
    pub next_run_at: DateTime<Utc>,
    pub overlap_policy: Option<String>,
}

pub struct UpdateProjectTaskScheduleCommand {
    pub schedule_id: i64,
    pub status: Option<String>,
    pub interval_seconds: Option<Option<i64>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub overlap_policy: Option<String>,
    pub template_payload: Option<ScheduleTemplatePayload>,
}

pub struct MaterializeProjectTaskScheduleCommand {
    pub schedule_id: i64,
}

pub struct ApplyScheduleStatePatchCommand {
    pub schedule_id: i64,
    pub expected_version: i64,
    pub patch: serde_json::Value,
}

impl CreateProjectTaskScheduleCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ProjectTaskSchedule, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        validate_schedule_kind(&self.schedule_kind, self.interval_seconds)?;
        let overlap_policy = self
            .overlap_policy
            .unwrap_or_else(|| models::project_task_schedule::overlap_policy::SKIP.to_string());
        validate_overlap_policy(&overlap_policy)?;
        let template_payload = serde_json::to_value(&self.template_payload).map_err(|e| {
            AppError::Internal(format!("Failed to serialize template_payload: {e}"))
        })?;

        let now = Utc::now();
        let schedule = sqlx::query_as!(
            ProjectTaskSchedule,
            r#"
            INSERT INTO project_task_schedules (
                id, board_id, task_key, template_payload, status, schedule_kind,
                interval_seconds, next_run_at, overlap_policy, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, 'active', $5,
                $6, $7, $8, $9, $9
            )
            RETURNING
                id, board_id, task_key, template_payload, state, state_version,
                status, schedule_kind, interval_seconds, next_run_at, last_fired_at,
                overlap_policy, created_at, updated_at
            "#,
            self.id,
            self.board_id,
            self.task_key,
            template_payload,
            self.schedule_kind,
            self.interval_seconds,
            self.next_run_at,
            overlap_policy,
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
            overlap_policy: None,
            template_payload: None,
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

    pub fn with_overlap_policy(mut self, overlap_policy: String) -> Self {
        self.overlap_policy = Some(overlap_policy);
        self
    }

    pub fn with_template_payload(mut self, template_payload: ScheduleTemplatePayload) -> Self {
        self.template_payload = Some(template_payload);
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ProjectTaskSchedule, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if let Some(policy) = &self.overlap_policy {
            validate_overlap_policy(policy)?;
        }
        let template_payload = self
            .template_payload
            .as_ref()
            .map(|p| {
                serde_json::to_value(p).map_err(|e| {
                    AppError::Internal(format!("Failed to serialize template_payload: {e}"))
                })
            })
            .transpose()?;

        let now = Utc::now();
        let schedule = sqlx::query_as!(
            ProjectTaskSchedule,
            r#"
            UPDATE project_task_schedules
            SET
                status = COALESCE($2, status),
                interval_seconds = COALESCE($3, interval_seconds),
                next_run_at = COALESCE($4, next_run_at),
                overlap_policy = COALESCE($5, overlap_policy),
                template_payload = COALESCE($6, template_payload),
                updated_at = $7
            WHERE id = $1
            RETURNING
                id, board_id, task_key, template_payload, state, state_version,
                status, schedule_kind, interval_seconds, next_run_at, last_fired_at,
                overlap_policy, created_at, updated_at
            "#,
            self.schedule_id,
            self.status,
            self.interval_seconds.flatten(),
            self.next_run_at,
            self.overlap_policy,
            template_payload,
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

    /// Returns the new instance's `board_item_id` when a fire was materialized,
    /// or `None` when the schedule was advanced without firing (skipped due to
    /// overlap policy, or already fired by another worker).
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Option<i64>, AppError>
    where
        D: HasDbRouter
            + HasIdProvider
            + HasNatsJetStreamProvider
            + common::HasNatsProvider
            + ?Sized,
    {
        let mut tx = deps.writer_pool().begin().await?;

        let row = sqlx::query!(
            r#"
            SELECT
                s.id              AS schedule_id,
                s.board_id        AS board_id,
                s.task_key        AS task_key,
                s.template_payload AS template_payload,
                s.state           AS state,
                s.state_version   AS state_version,
                s.schedule_kind   AS schedule_kind,
                s.interval_seconds AS interval_seconds,
                s.next_run_at     AS next_run_at,
                s.overlap_policy  AS overlap_policy
            FROM project_task_schedules s
            WHERE s.id = $1
              AND s.status = 'active'
              AND s.next_run_at <= NOW()
            FOR UPDATE SKIP LOCKED
            "#,
            self.schedule_id,
        )
        .fetch_optional(&mut *tx)
        .await?;

        let Some(row) = row else {
            println!(
                "[schedule_debug] schedule_id={} skipped: row not selectable (status != active, next_run_at > NOW(), or row locked)",
                self.schedule_id
            );
            tx.commit().await?;
            return Ok(None);
        };
        println!(
            "[schedule_debug] schedule_id={} picked up: next_run_at={} overlap_policy={} schedule_kind={} interval_seconds={:?}",
            self.schedule_id,
            row.next_run_at,
            row.overlap_policy,
            row.schedule_kind,
            row.interval_seconds
        );

        let scheduled_for = row.next_run_at;
        let now = Utc::now();

        if row.overlap_policy == models::project_task_schedule::overlap_policy::SKIP {
            let active = sqlx::query!(
                r#"
                SELECT COUNT(*)::BIGINT AS "count!"
                FROM project_task_board_items
                WHERE schedule_id = $1
                  AND archived_at IS NULL
                  AND status IN ('pending','available','claimed','in_progress')
                "#,
                self.schedule_id,
            )
            .fetch_one(&mut *tx)
            .await?;

            if active.count > 0 {
                let next = next_schedule_run_at(
                    &row.schedule_kind,
                    row.interval_seconds,
                    scheduled_for,
                    now,
                )?;

                advance_schedule(&mut *tx, self.schedule_id, next, now, false).await?;
                tx.commit().await?;
                return Ok(None);
            }
        }

        let new_item_id = deps.id_provider().next_id()? as i64;
        let task_key = format!("TASK-{}", new_item_id);
        let template: ScheduleTemplatePayload =
            serde_json::from_value(row.template_payload.clone())
                .map_err(|e| AppError::Internal(format!("Invalid template_payload: {e}")))?;
        let mut metadata: ProjectTaskBoardItemMetadata = template.metadata.clone();
        metadata.schedule_carryover = Some(ScheduleCarryover {
            schedule_id: self.schedule_id,
            state_snapshot: row.state.clone(),
            state_version: row.state_version,
            scheduled_for,
        });
        let metadata_value = serde_json::to_value(&metadata).map_err(|e| {
            AppError::Internal(format!("Failed to serialize new instance metadata: {e}"))
        })?;

        let inserted = sqlx::query!(
            r#"
            INSERT INTO project_task_board_items (
                id, board_id, task_key, title, description, status,
                assigned_thread_id, metadata, completed_at, archived_at,
                created_at, updated_at, state_version,
                schedule_id, scheduled_for, fired_at, pending_question
            ) VALUES (
                $1, $2, $3, $4, $5, 'pending',
                NULL, $6, NULL, NULL,
                $7, $7, 0,
                $8, $9, $7, NULL
            )
            ON CONFLICT (schedule_id, scheduled_for)
                WHERE schedule_id IS NOT NULL DO NOTHING
            RETURNING id
            "#,
            new_item_id,
            row.board_id,
            task_key,
            template.title,
            template.description,
            metadata_value,
            now,
            self.schedule_id,
            scheduled_for,
        )
        .fetch_optional(&mut *tx)
        .await?;

        let next =
            next_schedule_run_at(&row.schedule_kind, row.interval_seconds, scheduled_for, now)?;
        let did_fire = inserted.is_some();
        advance_schedule(&mut *tx, self.schedule_id, next, now, did_fire).await?;
        tx.commit().await?;

        let Some(_) = inserted else {
            println!(
                "[schedule_debug] schedule_id={} INSERT hit ON CONFLICT (already a row at scheduled_for={}). advanced next_run_at={:?}",
                self.schedule_id, scheduled_for, next
            );
            return Ok(None);
        };
        println!(
            "[schedule_debug] schedule_id={} FIRED new board_item_id={} task_key=TASK-{} scheduled_for={} next_run_at={:?}",
            self.schedule_id, new_item_id, new_item_id, scheduled_for, next
        );

        ReconcileProjectTaskBoardItemCommand::new(new_item_id)
            .with_note("Scheduled task fired; new instance materialized".to_string())
            .execute_with_deps(deps)
            .await?;

        Ok(Some(new_item_id))
    }
}

impl ApplyScheduleStatePatchCommand {
    pub fn new(schedule_id: i64, expected_version: i64, patch: serde_json::Value) -> Self {
        Self {
            schedule_id,
            expected_version,
            patch,
        }
    }

    /// Returns `Ok(true)` when the patch applied, `Ok(false)` on version conflict.
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<bool, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if !self.patch.is_object() {
            return Err(AppError::BadRequest(
                "schedule_state_patch must be a JSON object".to_string(),
            ));
        }

        let result = sqlx::query!(
            r#"
            UPDATE project_task_schedules
            SET state = state || $2::jsonb,
                state_version = state_version + 1,
                updated_at = NOW()
            WHERE id = $1
              AND state_version = $3
            "#,
            self.schedule_id,
            self.patch,
            self.expected_version,
        )
        .execute(executor)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}

/// Minimum allowed interval for recurring schedules. The dispatcher ticks
/// every 5 minutes, so anything shorter would either fire on every tick
/// regardless of intent, or miss its window entirely. 10 minutes gives a 2x
/// margin against tick alignment and clock skew.
pub const MIN_INTERVAL_SECONDS: i64 = 600;

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
            let secs = interval_seconds.unwrap_or(0);
            if secs <= 0 {
                return Err(AppError::BadRequest(
                    "interval schedules must set interval_seconds > 0".to_string(),
                ));
            }
            if secs < MIN_INTERVAL_SECONDS {
                return Err(AppError::BadRequest(format!(
                    "interval_seconds must be at least {MIN_INTERVAL_SECONDS} (10 minutes)"
                )));
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

fn validate_overlap_policy(policy: &str) -> Result<(), AppError> {
    match policy {
        models::project_task_schedule::overlap_policy::SKIP
        | models::project_task_schedule::overlap_policy::PARALLEL => Ok(()),
        other => Err(AppError::BadRequest(format!(
            "Unsupported overlap_policy '{}'",
            other
        ))),
    }
}

fn next_schedule_run_at(
    schedule_kind: &str,
    interval_seconds: Option<i64>,
    scheduled_for: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, AppError> {
    match schedule_kind {
        models::project_task_schedule::schedule_kind::ONCE => Ok(None),
        models::project_task_schedule::schedule_kind::INTERVAL => {
            let seconds = interval_seconds.ok_or_else(|| {
                AppError::BadRequest("interval schedule is missing interval_seconds".to_string())
            })?;
            let mut next = scheduled_for + ChronoDuration::seconds(seconds);
            while next <= now {
                next += ChronoDuration::seconds(seconds);
            }
            Ok(Some(next))
        }
        other => Err(AppError::BadRequest(format!(
            "Unsupported schedule_kind '{}'",
            other
        ))),
    }
}

async fn advance_schedule<'e, E>(
    executor: E,
    schedule_id: i64,
    next: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    fired: bool,
) -> Result<(), AppError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let next_status = if next.is_some() {
        models::project_task_schedule::status::ACTIVE
    } else {
        models::project_task_schedule::status::COMPLETED
    };

    let last_fired_at: Option<DateTime<Utc>> = if fired { Some(now) } else { None };

    sqlx::query!(
        r#"
        UPDATE project_task_schedules
        SET status = $2,
            next_run_at = COALESCE($3, next_run_at),
            last_fired_at = COALESCE($4, last_fired_at),
            updated_at = $5
        WHERE id = $1
        "#,
        schedule_id,
        next_status,
        next,
        last_fired_at,
        now,
    )
    .execute(executor)
    .await?;

    Ok(())
}
