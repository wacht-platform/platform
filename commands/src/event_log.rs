//! Event log + outbox commands.
//!
//! `event_log` is the unified audit log + outbox table introduced in Phase 1
//! of the event-driven hardening plan. State-mutation commands write rows
//! through `InsertEventLogCommand` inside their existing transaction; the
//! dispatcher (`worker/src/jobs/event_dispatcher.rs`) publishes them to NATS.
//!
//! Idempotency-key convention: `{event_type}_{aggregate_id}_{state_version}`.
//! Two concurrent emitters observing the same `state_version` produce
//! identical keys → the unique-index conflict makes the second insert a
//! silent no-op.
//!
//! See `docs/event-driven-hardening-plan.md`.

use chrono::{DateTime, Utc};
use common::error::AppError;
use serde_json::Value;
use sqlx::Postgres;

/// Status values for `event_log.publish_status`.
pub mod publish_status {
    pub const PENDING: &str = "pending";
    pub const PUBLISHING: &str = "publishing";
    pub const PUBLISHED: &str = "published";
    pub const FAILED: &str = "failed";
    pub const NO_PUBLISH: &str = "no_publish";
}

/// Aggregate types tracked in event_log. Free-form strings, but standardised
/// here so callers don't typo them.
pub mod aggregate_type {
    pub const THREAD: &str = "thread";
    pub const BOARD_ITEM: &str = "board_item";
    pub const ASSIGNMENT: &str = "assignment";
    pub const SCHEDULE: &str = "schedule";
}

/// NATS subject the dispatcher subscribes to. A publish here fast-paths the
/// dispatcher's drain loop (otherwise it waits up to 30s for the safety
/// poll). Body is ignored; the publish is a wake signal only.
pub const DISPATCHER_WAKE_SUBJECT: &str = "agent.outbox.wake";

/// NATS subject the worker consumes for `agent.event_log_work` tasks emitted
/// from event_log rows.
pub const EVENT_LOG_WORK_SUBJECT: &str = "worker.tasks.agent.event_log_work";

/// Fire-and-forget poke at the dispatcher. Failures are intentionally
/// swallowed: a missed nudge degrades to the 30s safety poll.
pub async fn nudge_dispatcher(nats: &async_nats::Client) {
    let _ = nats
        .publish(DISPATCHER_WAKE_SUBJECT.to_string(), Vec::new().into())
        .await;
}

/// Build the `event_log.payload` JSON for worker-consumed events.
///
/// The worker reads payloads by string key (`parse_payload_i64(&p, "event_log_id")`),
/// so producers and consumers don't share a typed struct. This builder
/// captures the four fields every payload sets — `event_log_id`,
/// `deployment_id`, `thread_id`, `kind` — encodes i64s as strings (the
/// worker's `parse_payload_i64` accepts string or number, but every existing
/// site emits string), and lets callers tack on event-specific extras.
pub struct EventLogPayload {
    map: serde_json::Map<String, Value>,
}

impl EventLogPayload {
    pub fn new(
        event_log_id: i64,
        deployment_id: i64,
        thread_id: i64,
        kind: impl Into<String>,
    ) -> Self {
        let mut map = serde_json::Map::with_capacity(4);
        map.insert("event_log_id".into(), Value::String(event_log_id.to_string()));
        map.insert("deployment_id".into(), Value::String(deployment_id.to_string()));
        map.insert("thread_id".into(), Value::String(thread_id.to_string()));
        map.insert("kind".into(), Value::String(kind.into()));
        Self { map }
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.map.insert(key.into(), value.into());
        self
    }

    pub fn with_id(self, key: impl Into<String>, id: i64) -> Self {
        self.with(key, Value::String(id.to_string()))
    }

    pub fn with_opt_id(self, key: impl Into<String>, id: Option<i64>) -> Self {
        self.with(
            key,
            id.map(|n| Value::String(n.to_string())).unwrap_or(Value::Null),
        )
    }

    pub fn with_serializable<T: serde::Serialize>(mut self, key: impl Into<String>, value: T) -> Self {
        if let Ok(v) = serde_json::to_value(value) {
            self.map.insert(key.into(), v);
        }
        self
    }

    pub fn build(self) -> Value {
        Value::Object(self.map)
    }

    pub fn as_object_mut(&mut self) -> &mut serde_json::Map<String, Value> {
        &mut self.map
    }
}

pub struct EnqueueThreadWorkEvent {
    pub event_log_id: i64,
    pub deployment_id: i64,
    pub thread_id: i64,
    pub event_type: String,
    pub priority: i32,
    pub agent_id: Option<i64>,
    pub conversation_id: Option<i64>,
    pub idempotency_key: String,
    pub execution_payload: Value,
}

impl EnqueueThreadWorkEvent {
    pub async fn execute<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = Postgres>,
    {
        let payload = EventLogPayload::new(
            self.event_log_id,
            self.deployment_id,
            self.thread_id,
            self.event_type.clone(),
        )
        .with_opt_id("agent_id", self.agent_id)
        .with_opt_id("conversation_id", self.conversation_id)
        .with("execution_payload", self.execution_payload.clone())
        .build();

        InsertEventLogCommand::new(
            self.event_log_id,
            self.deployment_id,
            aggregate_type::THREAD,
            self.thread_id,
            self.event_type,
            self.idempotency_key,
        )
        .with_payload(payload)
        .with_priority(self.priority)
        .with_publish_subject(EVENT_LOG_WORK_SUBJECT)
        .execute(executor)
        .await?;
        Ok(())
    }
}

/// Insert a row into `event_log` inside an existing transaction.
///
/// Uses `INSERT ... ON CONFLICT (idempotency_key) DO NOTHING` — re-inserts
/// with the same key are silent no-ops. Returns `Some(id)` on insert,
/// `None` on conflict.
pub struct InsertEventLogCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub aggregate_type: String,
    pub aggregate_id: i64,
    pub event_type: String,
    pub payload: Value,
    pub priority: i32,
    pub publish_subject: Option<String>,
    pub idempotency_key: String,
    pub actor_id: Option<i64>,
    pub caused_by_event_id: Option<i64>,
}

impl InsertEventLogCommand {
    pub fn new(
        id: i64,
        deployment_id: i64,
        aggregate_type: impl Into<String>,
        aggregate_id: i64,
        event_type: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Self {
        Self {
            id,
            deployment_id,
            aggregate_type: aggregate_type.into(),
            aggregate_id,
            event_type: event_type.into(),
            payload: Value::Object(Default::default()),
            priority: 100,
            publish_subject: None,
            idempotency_key: idempotency_key.into(),
            actor_id: None,
            caused_by_event_id: None,
        }
    }

    pub fn with_payload(mut self, payload: Value) -> Self {
        self.payload = payload;
        self
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_publish_subject(mut self, subject: impl Into<String>) -> Self {
        self.publish_subject = Some(subject.into());
        self
    }

    pub fn with_actor_id(mut self, actor_id: i64) -> Self {
        self.actor_id = Some(actor_id);
        self
    }

    pub fn with_caused_by_event_id(mut self, event_id: i64) -> Self {
        self.caused_by_event_id = Some(event_id);
        self
    }

    /// Execute against any executor (typically `&mut Transaction`).
    /// Returns `Some(id)` if the row was inserted, `None` if a row with the
    /// same `idempotency_key` already existed (silent dedup).
    pub async fn execute<'e, E>(self, executor: E) -> Result<Option<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = Postgres>,
    {
        let publish_status = if self.publish_subject.is_some() {
            publish_status::PENDING
        } else {
            publish_status::NO_PUBLISH
        };

        let row = sqlx::query!(
            r#"
            INSERT INTO event_log (
                id, deployment_id,
                aggregate_type, aggregate_id, event_type, payload, priority,
                publish_subject, publish_status,
                idempotency_key,
                actor_id, caused_by_event_id
            ) VALUES (
                $1, $2,
                $3, $4, $5, $6, $7,
                $8, $9,
                $10,
                $11, $12
            )
            ON CONFLICT (idempotency_key) DO NOTHING
            RETURNING id
            "#,
            self.id,
            self.deployment_id,
            self.aggregate_type,
            self.aggregate_id,
            self.event_type,
            self.payload,
            self.priority,
            self.publish_subject,
            publish_status,
            self.idempotency_key,
            self.actor_id,
            self.caused_by_event_id,
        )
        .fetch_optional(executor)
        .await?;

        if row.is_none() && self.publish_subject.is_some() {
            tracing::warn!(
                aggregate_type = %self.aggregate_type,
                aggregate_id = self.aggregate_id,
                event_type = %self.event_type,
                idempotency_key = %self.idempotency_key,
                "event_log INSERT hit ON CONFLICT; wake will not fire for this attempt (existing row may have already been published)"
            );
        }

        Ok(row.map(|r| r.id))
    }
}

// =============================================================================
// Dispatcher-side commands.
// =============================================================================

/// Row claimed by the dispatcher for publishing.
#[derive(Debug, Clone)]
pub struct ClaimedEvent {
    pub id: i64,
    pub deployment_id: i64,
    pub event_type: String,
    pub publish_subject: String,
    pub payload: Value,
    pub publish_attempts: i32,
}

/// Atomically claim up to `limit` pending or stuck-publishing rows.
///
/// Selects rows where:
///   - publish_status='pending' AND next_publish_at <= now() (normal pending), OR
///   - publish_status='publishing' AND publishing_started_at < now() - 60s
///     (dispatcher crashed mid-publish; recover the row).
///
/// Marks them publishing and increments publish_attempts. Uses
/// `FOR UPDATE SKIP LOCKED` so multiple dispatcher instances partition cleanly.
pub async fn claim_pending_events<'e, E>(
    executor: E,
    limit: i64,
) -> Result<Vec<ClaimedEvent>, AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let rows = sqlx::query!(
        r#"
        UPDATE event_log
        SET publish_status = 'publishing',
            publishing_started_at = NOW(),
            publish_attempts = publish_attempts + 1
        WHERE id = ANY (
            SELECT id FROM event_log
            WHERE publish_subject IS NOT NULL
              AND (
                  (publish_status = 'pending' AND next_publish_at <= NOW())
                  OR (publish_status = 'publishing' AND publishing_started_at < NOW() - INTERVAL '60 seconds')
              )
            ORDER BY priority ASC, next_publish_at ASC
            FOR UPDATE SKIP LOCKED
            LIMIT $1
        )
        RETURNING
            id AS "id!",
            deployment_id AS "deployment_id!",
            event_type AS "event_type!",
            publish_subject AS "publish_subject!",
            payload AS "payload!",
            publish_attempts AS "publish_attempts!"
        "#,
        limit,
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ClaimedEvent {
            id: r.id,
            deployment_id: r.deployment_id,
            event_type: r.event_type,
            publish_subject: r.publish_subject,
            payload: r.payload,
            publish_attempts: r.publish_attempts,
        })
        .collect())
}

/// Earliest `next_publish_at` among rows currently scheduled for the future.
/// Used by the dispatcher to set a single wake timer instead of polling.
/// Returns `None` if no future-scheduled events exist.
pub async fn next_pending_publish_at<'e, E>(
    executor: E,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let row = sqlx::query!(
        r#"
        SELECT MIN(next_publish_at) AS "earliest"
        FROM event_log
        WHERE publish_subject IS NOT NULL
          AND publish_status = 'pending'
          AND next_publish_at > NOW()
        "#,
    )
    .fetch_one(executor)
    .await?;
    Ok(row.earliest)
}

/// Mark an event as successfully published.
pub async fn mark_event_published<'e, E>(executor: E, event_id: i64) -> Result<(), AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    sqlx::query!(
        r#"
        UPDATE event_log
        SET publish_status = 'published',
            published_at = NOW(),
            publishing_started_at = NULL,
            last_publish_error = NULL
        WHERE id = $1
        "#,
        event_id,
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Reset an event back to pending after a transient publish failure.
/// Schedules retry with exponential backoff.
pub async fn schedule_event_retry<'e, E>(
    executor: E,
    event_id: i64,
    attempts: i32,
    error: &str,
) -> Result<(), AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let backoff_seconds = exponential_backoff_seconds(attempts);
    let interval = format!("{} seconds", backoff_seconds);
    sqlx::query!(
        r#"
        UPDATE event_log
        SET publish_status = 'pending',
            publishing_started_at = NULL,
            next_publish_at = NOW() + ($1::text)::interval,
            last_publish_error = $2
        WHERE id = $3
        "#,
        interval,
        error,
        event_id,
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Mark event as permanently failed. Caller decides when to do this
/// (typically when publish_attempts exceeds a threshold).
pub async fn mark_event_failed<'e, E>(
    executor: E,
    event_id: i64,
    error: &str,
) -> Result<(), AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    sqlx::query!(
        r#"
        UPDATE event_log
        SET publish_status = 'failed',
            publishing_started_at = NULL,
            last_publish_error = $1
        WHERE id = $2
        "#,
        error,
        event_id,
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Mark an event's work as completed (worker finished executing it).
pub async fn mark_event_work_completed<'e, E>(
    executor: E,
    event_id: i64,
    completed_at: DateTime<Utc>,
) -> Result<(), AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    sqlx::query!(
        r#"
        UPDATE event_log
        SET completed_at = $1
        WHERE id = $2 AND completed_at IS NULL
        "#,
        completed_at,
        event_id,
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Exponential backoff schedule (seconds): 1, 4, 16, 64, 256, 1024, 4096, 16384.
/// Total ~24 hours across 8 attempts.
pub fn exponential_backoff_seconds(attempts: i32) -> i64 {
    let exp = attempts.saturating_sub(1).clamp(0, 7);
    4i64.pow(exp as u32)
}

/// Number of attempts before an event is dead-lettered.
pub const MAX_PUBLISH_ATTEMPTS: i32 = 8;

// =============================================================================
// work_lease commands.
// =============================================================================

/// Try to claim a lease on an event. Returns `Ok(true)` if claimed,
/// `Ok(false)` if someone else already holds it. Workers call this after
/// receiving a NATS-delivered event; if claim fails, ACK NATS and exit.
pub async fn claim_work_lease<'e, E>(
    executor: E,
    event_id: i64,
    worker_id: &str,
    lease_seconds: i64,
) -> Result<bool, AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let interval = format!("{} seconds", lease_seconds);
    let row = sqlx::query!(
        r#"
        INSERT INTO work_lease (event_id, worker_id, expires_at)
        VALUES ($1, $2, NOW() + ($3::text)::interval)
        ON CONFLICT (event_id) DO NOTHING
        RETURNING event_id
        "#,
        event_id,
        worker_id,
        interval,
    )
    .fetch_optional(executor)
    .await?;
    Ok(row.is_some())
}

/// Extend the lease while work is in progress. Should be called every
/// ~half of the lease duration.
pub async fn heartbeat_work_lease<'e, E>(
    executor: E,
    event_id: i64,
    worker_id: &str,
    lease_seconds: i64,
) -> Result<bool, AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let interval = format!("{} seconds", lease_seconds);
    let row = sqlx::query!(
        r#"
        UPDATE work_lease
        SET heartbeat_at = NOW(),
            expires_at = NOW() + ($3::text)::interval,
            attempts = attempts + 1
        WHERE event_id = $1 AND worker_id = $2
        RETURNING event_id
        "#,
        event_id,
        worker_id,
        interval,
    )
    .fetch_optional(executor)
    .await?;
    Ok(row.is_some())
}

/// Release the lease (work completed cleanly).
pub async fn release_work_lease<'e, E>(executor: E, event_id: i64) -> Result<(), AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    sqlx::query!("DELETE FROM work_lease WHERE event_id = $1", event_id,)
        .execute(executor)
        .await?;
    Ok(())
}

/// Recover expired leases that still have retries left. Returns the
/// event_ids that were reset to 'pending' for republish.
pub async fn reclaim_expired_leases<'e, E>(
    executor: E,
    max_attempts: i32,
) -> Result<Vec<i64>, AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let rows = sqlx::query!(
        r#"
        WITH expired AS (
            DELETE FROM work_lease
            WHERE expires_at < NOW()
              AND attempts < $1
            RETURNING event_id
        )
        UPDATE event_log
        SET publish_status = 'pending',
            next_publish_at = NOW(),
            publishing_started_at = NULL
        WHERE id IN (SELECT event_id FROM expired)
        RETURNING id AS "id!"
        "#,
        max_attempts,
    )
    .fetch_all(executor)
    .await?;
    Ok(rows.into_iter().map(|r| r.id).collect())
}

/// Mark events that exhausted their lease retry budget as failed.
/// Returns the number of rows affected.
pub async fn mark_exhausted_leases_failed<'e, E>(
    executor: E,
    max_attempts: i32,
) -> Result<u64, AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let result = sqlx::query!(
        r#"
        WITH exhausted AS (
            DELETE FROM work_lease
            WHERE expires_at < NOW()
              AND attempts >= $1
            RETURNING event_id
        )
        UPDATE event_log
        SET publish_status = 'failed',
            publishing_started_at = NULL,
            last_publish_error = 'lease retries exhausted'
        WHERE id IN (SELECT event_id FROM exhausted)
        "#,
        max_attempts,
    )
    .execute(executor)
    .await?;
    Ok(result.rows_affected())
}

/// Default lease duration for an executing event.
pub const DEFAULT_LEASE_SECONDS: i64 = 300; // 5 minutes
/// Lease retry cap before dead-letter.
pub const MAX_LEASE_ATTEMPTS: i32 = 5;

#[derive(Debug, Clone)]
pub struct StuckAssignmentRow {
    pub assignment_id: i64,
    pub board_item_id: i64,
    pub thread_id: i64,
    pub status: String,
    pub stale_seconds: i64,
}

/// Find assignments stuck in `claimed`/`in_progress` past `stale_seconds`
/// with no active `work_lease`. Used by the worker's stuck-assignment
/// observability sweeper.
pub async fn list_stuck_assignments<'e, E>(
    executor: E,
    stale_seconds: i64,
    limit: i64,
) -> Result<Vec<StuckAssignmentRow>, AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let rows = sqlx::query!(
        r#"
        SELECT a.id,
               a.board_item_id,
               a.thread_id,
               a.status,
               EXTRACT(EPOCH FROM (NOW() - a.updated_at))::bigint AS "stale_seconds!"
        FROM project_task_board_item_assignments a
        WHERE a.status IN ('claimed', 'in_progress')
          AND a.assignment_role <> 'coordinator'
          AND a.updated_at < NOW() - INTERVAL '1 second' * $1
          AND NOT EXISTS (
              SELECT 1
              FROM event_log el
              INNER JOIN work_lease wl ON wl.event_id = el.id
              WHERE el.aggregate_type = 'assignment'
                AND el.aggregate_id = a.id
                AND wl.expires_at > NOW()
          )
        ORDER BY a.updated_at ASC
        LIMIT $2
        "#,
        stale_seconds as f64,
        limit,
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| StuckAssignmentRow {
            assignment_id: r.id,
            board_item_id: r.board_item_id,
            thread_id: r.thread_id,
            status: r.status,
            stale_seconds: r.stale_seconds,
        })
        .collect())
}
