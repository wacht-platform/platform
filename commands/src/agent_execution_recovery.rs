use chrono::Utc;
use common::{HasDbRouter, HasIdProvider, error::AppError};
use models::AgentExecutionRecoveryEntry;

pub struct RecordAgentExecutionRecoveryCommand {
    pub thread_id: i64,
    pub thread_event_id: Option<i64>,
    pub execution_run_id: Option<i64>,
    pub reason_code: String,
    pub reason_detail: serde_json::Value,
}

pub struct UpdateAgentExecutionRecoveryStatusCommand {
    pub entry_id: i64,
    pub status: Option<String>,
    pub reason_detail: Option<serde_json::Value>,
    pub increment_retry_count: bool,
    pub mark_attempted_now: bool,
    pub mark_resolved_now: bool,
}

impl RecordAgentExecutionRecoveryCommand {
    pub async fn execute_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<AgentExecutionRecoveryEntry, AppError>
    where
        D: HasDbRouter + HasIdProvider + ?Sized,
    {
        let now = Utc::now();
        let mut tx = deps.writer_pool().begin().await?;

        let existing = sqlx::query_as!(
            AgentExecutionRecoveryEntry,
            r#"
            SELECT
                id, thread_id, thread_event_id, execution_run_id,
                reason_code, reason_detail, status, retry_count, last_recovery_attempt_at,
                resolved_at, created_at, updated_at
            FROM agent_execution_recovery_queue
            WHERE thread_id = $1
              AND thread_event_id IS NOT DISTINCT FROM $2
              AND execution_run_id IS NOT DISTINCT FROM $3
              AND reason_code = $4
              AND resolved_at IS NULL
            ORDER BY created_at DESC
            LIMIT 1
            FOR UPDATE
            "#,
            self.thread_id,
            self.thread_event_id,
            self.execution_run_id,
            self.reason_code,
        )
        .fetch_optional(&mut *tx)
        .await?;

        let row = if let Some(existing) = existing {
            sqlx::query_as!(
                AgentExecutionRecoveryEntry,
                r#"
                UPDATE agent_execution_recovery_queue
                SET
                    reason_detail = $2,
                    updated_at = $3
                WHERE id = $1
                RETURNING
                    id, thread_id, thread_event_id, execution_run_id,
                    reason_code, reason_detail, status, retry_count, last_recovery_attempt_at,
                    resolved_at, created_at, updated_at
                "#,
                existing.id,
                self.reason_detail,
                now,
            )
            .fetch_one(&mut *tx)
            .await?
        } else {
            let id = deps.id_provider().next_id()? as i64;
            sqlx::query_as!(
                AgentExecutionRecoveryEntry,
                r#"
                INSERT INTO agent_execution_recovery_queue (
                    id, thread_id, thread_event_id, execution_run_id,
                    reason_code, reason_detail, status, retry_count, created_at, updated_at
                )
                VALUES (
                    $1, $2, $3, $4,
                    $5, $6, 'open', 0, $7, $7
                )
                RETURNING
                    id, thread_id, thread_event_id, execution_run_id,
                    reason_code, reason_detail, status, retry_count, last_recovery_attempt_at,
                    resolved_at, created_at, updated_at
                "#,
                id,
                self.thread_id,
                self.thread_event_id,
                self.execution_run_id,
                self.reason_code,
                self.reason_detail,
                now,
            )
            .fetch_one(&mut *tx)
            .await?
        };

        tx.commit().await?;
        Ok(row)
    }
}

impl UpdateAgentExecutionRecoveryStatusCommand {
    pub fn new(entry_id: i64) -> Self {
        Self {
            entry_id,
            status: None,
            reason_detail: None,
            increment_retry_count: false,
            mark_attempted_now: false,
            mark_resolved_now: false,
        }
    }

    pub fn with_status(mut self, status: String) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_reason_detail(mut self, reason_detail: serde_json::Value) -> Self {
        self.reason_detail = Some(reason_detail);
        self
    }

    pub fn increment_retry_count(mut self) -> Self {
        self.increment_retry_count = true;
        self
    }

    pub fn mark_attempted_now(mut self) -> Self {
        self.mark_attempted_now = true;
        self
    }

    pub fn mark_resolved_now(mut self) -> Self {
        self.mark_resolved_now = true;
        self
    }

    pub async fn execute_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<AgentExecutionRecoveryEntry, AppError>
    where
        D: HasDbRouter + ?Sized,
    {
        let now = Utc::now();
        let row = sqlx::query_as!(
            AgentExecutionRecoveryEntry,
            r#"
            UPDATE agent_execution_recovery_queue
            SET
                status = COALESCE($2, status),
                reason_detail = COALESCE($3, reason_detail),
                retry_count = CASE WHEN $4 THEN retry_count + 1 ELSE retry_count END,
                last_recovery_attempt_at = CASE
                    WHEN $5 THEN $1
                    ELSE last_recovery_attempt_at
                END,
                resolved_at = CASE
                    WHEN $6 THEN $1
                    ELSE resolved_at
                END,
                updated_at = $1
            WHERE id = $7
            RETURNING
                id, thread_id, thread_event_id, execution_run_id,
                reason_code, reason_detail, status, retry_count, last_recovery_attempt_at,
                resolved_at, created_at, updated_at
            "#,
            now,
            self.status,
            self.reason_detail,
            self.increment_retry_count,
            self.mark_attempted_now,
            self.mark_resolved_now,
            self.entry_id,
        )
        .fetch_one(deps.writer_pool())
        .await?;

        Ok(row)
    }
}
