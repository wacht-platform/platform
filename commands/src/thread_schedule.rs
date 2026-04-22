use common::{HasDbRouter, error::AppError};
use models::ThreadEvent;
use std::str::FromStr;

use crate::{ThreadEventWakeDisposition, wake_disposition_for_thread_event};

pub struct ClaimNextSchedulableThreadEventCommand {
    deployment_id: i64,
    thread_id: i64,
}

pub enum ClaimNextSchedulableThreadEventResult {
    Claimed(ThreadEvent),
    NoThreadAvailable,
    NoPendingEvent,
    WakeNotAllowed {
        event_id: i64,
        event_type: String,
        thread_status: String,
    },
}

pub struct ReleaseClaimedThreadEventCommand {
    event_id: i64,
}

impl ClaimNextSchedulableThreadEventCommand {
    pub fn new(deployment_id: i64, thread_id: i64) -> Self {
        Self {
            deployment_id,
            thread_id,
        }
    }

    pub async fn execute_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<ClaimNextSchedulableThreadEventResult, AppError>
    where
        D: HasDbRouter + ?Sized,
    {
        let mut tx = deps.writer_pool().begin().await?;

        let thread_row = sqlx::query!(
            r#"
            SELECT status, reusable, accepts_assignments
            FROM agent_threads
            WHERE id = $1
              AND deployment_id = $2
              AND archived_at IS NULL
            "#,
            self.thread_id,
            self.deployment_id,
        )
        .fetch_optional(&mut *tx)
        .await?;

        let Some(thread_row) = thread_row else {
            tx.commit().await?;
            return Ok(ClaimNextSchedulableThreadEventResult::NoThreadAvailable);
        };

        let mut thread_status =
            models::AgentThreadStatus::from_str(&thread_row.status).map_err(|_| {
                AppError::Internal(format!(
                    "Invalid thread status for thread {}",
                    self.thread_id
                ))
            })?;

        let pending_row = sqlx::query_as!(
            ThreadEvent,
            r#"
            SELECT
                id, deployment_id, thread_id, board_item_id, event_type, status,
                priority, payload, available_at, claimed_at, completed_at, failed_at,
                caused_by_run_id, caused_by_thread_id, conversation_id, retry_count, max_retries, created_at, updated_at
            FROM thread_events
            WHERE thread_id = $1
              AND status = 'pending'
              AND available_at <= NOW()
            ORDER BY priority ASC, available_at ASC, created_at ASC
            LIMIT 1
            FOR UPDATE SKIP LOCKED
            "#,
            self.thread_id,
        )
        .fetch_optional(&mut *tx)
        .await?;

        let Some(pending_event) = pending_row else {
            tx.commit().await?;
            return Ok(ClaimNextSchedulableThreadEventResult::NoPendingEvent);
        };

        if wake_disposition_for_thread_event(
            &thread_status,
            &pending_event.event_type,
            thread_row.reusable,
            thread_row.accepts_assignments,
        ) != ThreadEventWakeDisposition::Published
        {
            tx.commit().await?;
            return Ok(ClaimNextSchedulableThreadEventResult::WakeNotAllowed {
                event_id: pending_event.id,
                event_type: pending_event.event_type,
                thread_status: thread_status.to_string(),
            });
        }

        let event = sqlx::query_as!(
            ThreadEvent,
            r#"
            UPDATE thread_events
            SET status = 'claimed', claimed_at = NOW(), updated_at = NOW()
            WHERE id = $1
            RETURNING
                id, deployment_id, thread_id, board_item_id, event_type, status,
                priority, payload, available_at, claimed_at, completed_at, failed_at,
                caused_by_run_id, caused_by_thread_id, conversation_id, retry_count, max_retries, created_at, updated_at
            "#,
            pending_event.id,
        )
        .fetch_one(&mut *tx)
        .await?;

        if matches!(thread_status, models::AgentThreadStatus::Failed)
            && wake_disposition_for_thread_event(
                &thread_status,
                &event.event_type,
                thread_row.reusable,
                thread_row.accepts_assignments,
            ) == ThreadEventWakeDisposition::Published
        {
            sqlx::query!(
                r#"
                UPDATE agent_threads
                SET status = 'interrupted', updated_at = NOW(), last_activity_at = NOW()
                WHERE id = $1 AND deployment_id = $2
                "#,
                self.thread_id,
                self.deployment_id,
            )
            .execute(&mut *tx)
            .await?;
            thread_status = models::AgentThreadStatus::Interrupted;
        }

        if wake_disposition_for_thread_event(
            &thread_status,
            &event.event_type,
            thread_row.reusable,
            thread_row.accepts_assignments,
        ) != ThreadEventWakeDisposition::Published
        {
            sqlx::query!(
                r#"
                UPDATE thread_events
                SET status = 'pending', claimed_at = NULL, updated_at = NOW()
                WHERE id = $1 AND caused_by_run_id IS NULL
                "#,
                event.id,
            )
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(ClaimNextSchedulableThreadEventResult::WakeNotAllowed {
                event_id: event.id,
                event_type: event.event_type,
                thread_status: thread_status.to_string(),
            });
        }

        tx.commit().await?;
        Ok(ClaimNextSchedulableThreadEventResult::Claimed(event))
    }
}

impl ReleaseClaimedThreadEventCommand {
    pub fn new(event_id: i64) -> Self {
        Self { event_id }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter + ?Sized,
    {
        sqlx::query!(
            r#"
            UPDATE thread_events
            SET status = 'pending', claimed_at = NULL, updated_at = NOW()
            WHERE id = $1 AND caused_by_run_id IS NULL
            "#,
            self.event_id,
        )
        .execute(deps.writer_pool())
        .await?;

        Ok(())
    }
}
