use chrono::Utc;
use common::{HasDbRouter, HasIdProvider, ReadConsistency, error::AppError};
use models::{
    ProjectTaskBoardItem, ProjectTaskBoardItemAssignment, TaskSubscriptionEventKind,
};
use queries::{ListProjectTaskBoardItemAssignmentsQuery, ListSubscribersForBoardItemQuery};
use serde::Serialize;
use sqlx::Postgres;

use crate::event_log::publish_status;

use crate::event_log;

#[derive(Debug, Clone, Serialize)]
pub struct TaskRoutingFieldChange {
    pub field: String,
    pub from: String,
    pub to: String,
}

pub fn build_task_routing_summary(
    board_item: &ProjectTaskBoardItem,
    prior_assignment_count: usize,
) -> String {
    format!(
        "Coordinator received routing signal for task #{} '{}' (status={}). {} prior assignment(s) on this task.",
        board_item.id, board_item.title, board_item.status, prior_assignment_count,
    )
}

pub fn build_assignment_execution_summary(
    assignment: &ProjectTaskBoardItemAssignment,
    board_item: &ProjectTaskBoardItem,
    total_siblings: usize,
    prior: Option<&ProjectTaskBoardItemAssignment>,
) -> String {
    let prior_desc = prior
        .map(|a| {
            let rs = a.result_status.as_deref().unwrap_or(a.status.as_str());
            let rs_summary = a.result_summary.as_deref().unwrap_or("(no summary)");
            format!(
                "prior assignment #{} (role={}, result_status={}, summary={})",
                a.id, a.assignment_role, rs, rs_summary,
            )
        })
        .unwrap_or_else(|| "this is the first assignment in the chain".to_string());
    format!(
        "Task #{} '{}' is now active on this thread. Assignment #{} transitioned to in_progress (role={}, {} of {}). {}.",
        board_item.id,
        board_item.title,
        assignment.id,
        assignment.assignment_role,
        prior.map(|_| total_siblings).unwrap_or(1),
        total_siblings,
        prior_desc,
    )
}

pub async fn fetch_assignment_siblings<D>(
    deps: &D,
    board_item_id: i64,
) -> Result<Vec<ProjectTaskBoardItemAssignment>, AppError>
where
    D: HasDbRouter + ?Sized,
{
    ListProjectTaskBoardItemAssignmentsQuery::new(board_item_id)
        .execute_with_db(deps.reader_pool(ReadConsistency::Strong))
        .await
}

/// Routing event_log row written when a board item needs the coordinator's
/// attention (creation, journal update, returned to coordinator). The shape
/// — payload schema, event_type, priority, publish_subject — is the same
/// across every call site, so the construction is centralised here.
pub struct InsertTaskRoutingEvent<'a> {
    pub event_log_id: i64,
    pub deployment_id: i64,
    pub coordinator_thread_id: i64,
    pub board_item: &'a ProjectTaskBoardItem,
    pub idempotency_key: String,
    pub summary: String,
    pub note: Option<String>,
    pub caused_by_event_id: Option<i64>,
    pub routing_reason: &'static str,
    pub previous_status: Option<String>,
    pub changed_fields: Vec<TaskRoutingFieldChange>,
    pub last_assignment_result_status: Option<String>,
}

impl<'a> InsertTaskRoutingEvent<'a> {
    pub async fn execute<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = Postgres>,
    {
        let mut payload = serde_json::json!({
            "event_log_id": self.event_log_id.to_string(),
            "deployment_id": self.deployment_id.to_string(),
            "thread_id": self.coordinator_thread_id.to_string(),
            "board_item_id": self.board_item.id.to_string(),
            "kind": "task_routing",
            "routing_reason": self.routing_reason,
            "summary": self.summary,
            "note": self.note,
        });
        if let Some(map) = payload.as_object_mut() {
            if let Some(prev) = self
                .previous_status
                .filter(|s| !s.is_empty() && s != &self.board_item.status)
            {
                map.insert("previous_status".to_string(), serde_json::Value::String(prev));
            }
            if !self.changed_fields.is_empty() {
                map.insert(
                    "changed_fields".to_string(),
                    serde_json::to_value(&self.changed_fields).unwrap_or(serde_json::Value::Null),
                );
            }
            if let Some(last) = self.last_assignment_result_status.filter(|s| !s.is_empty()) {
                map.insert(
                    "last_assignment_result_status".to_string(),
                    serde_json::Value::String(last),
                );
            }
        }

        sqlx::query!(
            r#"
            WITH coalesced AS (
                UPDATE event_log
                SET payload = $1::jsonb,
                    next_publish_at = NOW()
                WHERE aggregate_type = 'board_item'
                  AND aggregate_id = $2
                  AND event_type = 'task_routing'
                  AND publish_status = $3
                RETURNING id
            )
            INSERT INTO event_log (
                id, deployment_id,
                aggregate_type, aggregate_id, event_type, payload, priority,
                publish_subject, publish_status,
                idempotency_key,
                caused_by_event_id
            )
            SELECT
                $4, $5,
                'board_item', $2, 'task_routing', $1::jsonb, 15,
                $6, $3,
                $7,
                $8
            WHERE NOT EXISTS (SELECT 1 FROM coalesced)
            ON CONFLICT (idempotency_key) DO NOTHING
            "#,
            payload,
            self.board_item.id,
            publish_status::PENDING,
            self.event_log_id,
            self.deployment_id,
            event_log::EVENT_LOG_WORK_SUBJECT,
            self.idempotency_key,
            self.caused_by_event_id,
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

/// Mark every claimed/in_progress assignment for a board item as
/// cancelled+preempted. Returns true when at least one row was updated.
/// Mirrors the frontend-api `preemptActiveBoardItemAssignment` flow used on
/// task content edits and user comments.
pub async fn preempt_active_board_item_assignments<'e, E>(
    executor: E,
    board_item_id: i64,
    summary: &str,
) -> Result<bool, AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let result = sqlx::query!(
        r#"
        UPDATE project_task_board_item_assignments
        SET status = 'cancelled',
            result_status = 'preempted',
            completed_at = NOW(),
            result_summary = $2,
            updated_at = NOW()
        WHERE board_item_id = $1
          AND status IN ('claimed', 'in_progress')
        "#,
        board_item_id,
        summary,
    )
    .execute(executor)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Mark pending `task_routing` event_log rows for a board item as published,
/// so the dispatcher skips them. Used when a task is cancelled and any
/// queued routing signal is no longer relevant.
pub async fn suppress_pending_task_routing<'e, E>(
    executor: E,
    board_item_id: i64,
) -> Result<(), AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    sqlx::query!(
        r#"
        UPDATE event_log
        SET publish_status = 'published',
            next_publish_at = NULL
        WHERE aggregate_type = 'board_item'
          AND aggregate_id = $1
          AND event_type = 'task_routing'
          AND publish_status = 'pending'
        "#,
        board_item_id,
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Apply a REST-style edit (title / description / status) to a board item:
/// computes the field diff, writes the columns, preempts running
/// assignments on content edits or status->cancelled, and re-routes the
/// coordinator. Shared by the public REST endpoint and conversation-thread
/// agent calls so both paths produce identical preempt + route behaviour.
pub struct ApplyBoardItemEditCommand<'a> {
    pub deployment_id: i64,
    pub board_item_id: i64,
    pub coordinator_thread_id: Option<i64>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub preempt_summary: &'a str,
    pub fanout_subscriptions: bool,
}

pub struct BoardItemEditOutcome {
    pub item: ProjectTaskBoardItem,
    pub changed_fields: Vec<TaskRoutingFieldChange>,
    pub preempted: bool,
    pub routed: bool,
    pub subscribers_notified: usize,
}

impl<'a> ApplyBoardItemEditCommand<'a> {
    pub async fn execute<D>(self, deps: &D) -> Result<BoardItemEditOutcome, AppError>
    where
        D: HasDbRouter + HasIdProvider + ?Sized,
    {
        let pool = deps.writer_pool();
        let original = sqlx::query_as!(
            ProjectTaskBoardItem,
            r#"
            SELECT id, board_id, task_key, title, description, status,
                   assigned_thread_id, metadata, completed_at, archived_at,
                   created_at, updated_at, state_version,
                   schedule_id, scheduled_for, fired_at,
                   pending_question, pending_approval, mounts, exclusive_owner_agent_id
            FROM project_task_board_items
            WHERE id = $1 AND archived_at IS NULL
            "#,
            self.board_item_id,
        )
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Project task board item not found".to_string()))?;

        let original_status = original.status.clone();
        let original_title = original.title.clone();
        let original_description = original.description.clone().unwrap_or_default();

        let new_title = self
            .title
            .as_ref()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let new_description = self.description.as_ref().map(|v| v.trim().to_string());
        let new_status = self
            .status
            .as_ref()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());

        let mut changed_fields: Vec<TaskRoutingFieldChange> = Vec::new();
        if let Some(t) = new_title.as_ref() {
            if t != &original_title {
                changed_fields.push(TaskRoutingFieldChange {
                    field: "title".to_string(),
                    from: original_title.clone(),
                    to: t.clone(),
                });
            }
        }
        if let Some(d) = new_description.as_ref() {
            if d != &original_description {
                changed_fields.push(TaskRoutingFieldChange {
                    field: "description".to_string(),
                    from: original_description.clone(),
                    to: d.clone(),
                });
            }
        }
        if let Some(s) = new_status.as_ref() {
            if s != &original_status {
                changed_fields.push(TaskRoutingFieldChange {
                    field: "status".to_string(),
                    from: original_status.clone(),
                    to: s.clone(),
                });
            }
        }

        if changed_fields.is_empty() {
            return Ok(BoardItemEditOutcome {
                item: original,
                changed_fields,
                preempted: false,
                routed: false,
                subscribers_notified: 0,
            });
        }

        let content_changed = changed_fields
            .iter()
            .any(|c| c.field == "title" || c.field == "description");
        let cancelled_now = matches!(new_status.as_deref(), Some("cancelled"))
            && original_status != "cancelled";

        let mut tx = pool.begin().await?;

        let title_param = new_title.as_deref();
        let description_param = new_description.as_deref();
        let description_present = new_description.is_some();
        let status_param = new_status.as_deref();
        let item = sqlx::query_as!(
            ProjectTaskBoardItem,
            r#"
            UPDATE project_task_board_items
            SET title = COALESCE($2, title),
                description = CASE WHEN $4::boolean THEN $3 ELSE description END,
                status = COALESCE($5, status),
                completed_at = CASE
                    WHEN $5 = 'completed' THEN NOW()
                    WHEN $5 IS NOT NULL AND $5 <> 'completed' THEN NULL
                    ELSE completed_at
                END,
                updated_at = NOW()
            WHERE id = $1 AND archived_at IS NULL
            RETURNING id, board_id, task_key, title, description, status,
                      assigned_thread_id, metadata, completed_at, archived_at,
                      created_at, updated_at, state_version,
                      schedule_id, scheduled_for, fired_at,
                      pending_question, pending_approval, mounts, exclusive_owner_agent_id
            "#,
            self.board_item_id,
            title_param,
            description_param,
            description_present,
            status_param,
        )
        .fetch_one(&mut *tx)
        .await?;

        let preempted = if content_changed || cancelled_now {
            preempt_active_board_item_assignments(&mut *tx, item.id, self.preempt_summary).await?
        } else {
            false
        };

        let mut routed = false;
        if cancelled_now {
            suppress_pending_task_routing(&mut *tx, item.id).await?;
        } else if let Some(coordinator_thread_id) = self.coordinator_thread_id {
            let routing_reason = if preempted {
                models::thread_event::routing_reason::ASSIGNMENT_PREEMPTED
            } else {
                models::thread_event::routing_reason::TASK_UPDATED
            };
            let summary = format!(
                "Coordinator received {} signal for task #{} '{}' (status={}).",
                routing_reason, item.id, item.title, item.status,
            );
            let routing_event_id = deps.id_provider().next_id()? as i64;
            InsertTaskRoutingEvent {
                event_log_id: routing_event_id,
                deployment_id: self.deployment_id,
                coordinator_thread_id,
                board_item: &item,
                idempotency_key: format!(
                    "task_routing_{}_{}_{}",
                    item.id, item.state_version, routing_event_id
                ),
                summary,
                note: None,
                caused_by_event_id: None,
                routing_reason,
                previous_status: Some(original_status.clone()),
                changed_fields: changed_fields.clone(),
                last_assignment_result_status: None,
            }
            .execute(&mut *tx)
            .await?;
            routed = true;
        }

        let mut subscribers_notified = 0usize;
        if self.fanout_subscriptions {
            if let Some(kind) = TaskSubscriptionEventKind::from_status(&item.status) {
                if item.status != original_status {
                    subscribers_notified = fan_out_task_subscription_notifications(
                        &mut tx,
                        deps,
                        self.deployment_id,
                        &item,
                        &original_status,
                        kind,
                        Utc::now(),
                    )
                    .await?;
                }
            }
        }

        if cancelled_now {
            crate::DeleteSubscriptionsForBoardItemCommand {
                board_item_id: item.id,
            }
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        Ok(BoardItemEditOutcome {
            item,
            changed_fields,
            preempted,
            routed,
            subscribers_notified,
        })
    }
}

pub const SUBSCRIPTION_DEBOUNCE_SECONDS: i64 = 30;

pub async fn fan_out_task_subscription_notifications<D>(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    deps: &D,
    deployment_id: i64,
    board_item: &ProjectTaskBoardItem,
    from_status: &str,
    event_kind: TaskSubscriptionEventKind,
    transitioned_at: chrono::DateTime<Utc>,
) -> Result<usize, AppError>
where
    D: HasIdProvider + ?Sized,
{
    let subscribers = ListSubscribersForBoardItemQuery::new(board_item.id, event_kind.as_str())
        .execute_with_db(&mut **tx)
        .await?;
    if subscribers.is_empty() {
        return Ok(0);
    }

    let interval = format!("{} seconds", SUBSCRIPTION_DEBOUNCE_SECONDS);

    for sub in &subscribers {
        let record_id = deps.id_provider().next_id()? as i64;
        let content = serde_json::json!({
            "type": "task_subscription_notification",
            "board_item_id": board_item.id.to_string(),
            "task_key": board_item.task_key,
            "task_title": board_item.title,
            "from_status": from_status,
            "to_status": board_item.status,
            "transitioned_at": transitioned_at.to_rfc3339(),
        });
        let metadata = serde_json::json!({
            "subscription_event_kind": event_kind.as_str(),
            "consumed_at": serde_json::Value::Null,
        });
        sqlx::query!(
            r#"
            INSERT INTO conversations (
                id, thread_id, board_item_id, execution_run_id, timestamp,
                content, message_type, created_at, updated_at, metadata
            ) VALUES (
                $1, $2, $3, NULL, NOW(), $4::jsonb, 'task_subscription_notification',
                NOW(), NOW(), $5::jsonb
            )
            "#,
            record_id,
            sub.thread_id,
            board_item.id,
            content,
            metadata,
        )
        .execute(&mut **tx)
        .await?;

        let wake_event_id = deps.id_provider().next_id()? as i64;
        let payload = serde_json::json!({
            "event_log_id": wake_event_id.to_string(),
            "deployment_id": deployment_id.to_string(),
            "thread_id": sub.thread_id.to_string(),
            "kind": "thread_subscription_delivery",
            "board_item_id": board_item.id.to_string(),
        });
        let idempotency_key = format!(
            "thread_subscription_delivery_{}_{}",
            sub.thread_id, wake_event_id
        );

        sqlx::query!(
            r#"
            WITH coalesced AS (
                UPDATE event_log
                SET payload = $1::jsonb
                WHERE aggregate_type = 'thread'
                  AND aggregate_id = $3
                  AND event_type = 'thread_subscription_delivery'
                  AND publish_status = 'pending'
                  AND deployment_id = $5
                RETURNING id
            )
            INSERT INTO event_log (
                id, deployment_id,
                aggregate_type, aggregate_id, event_type, payload, priority,
                publish_subject, publish_status, next_publish_at,
                idempotency_key
            )
            SELECT
                $4, $5,
                'thread', $3, 'thread_subscription_delivery', $1::jsonb, 30,
                $6, 'pending', NOW() + ($2::text)::interval,
                $7
            WHERE NOT EXISTS (SELECT 1 FROM coalesced)
            ON CONFLICT (idempotency_key) DO NOTHING
            "#,
            payload,
            interval,
            sub.thread_id,
            wake_event_id,
            deployment_id,
            event_log::EVENT_LOG_WORK_SUBJECT,
            idempotency_key,
        )
        .execute(&mut **tx)
        .await?;
    }

    Ok(subscribers.len())
}

pub async fn mark_subscription_notifications_consumed<'e, E>(
    executor: E,
    thread_id: i64,
) -> Result<u64, AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let res = sqlx::query!(
        r#"
        UPDATE conversations
        SET metadata = jsonb_set(
                COALESCE(metadata, '{}'::jsonb),
                '{consumed_at}',
                to_jsonb(NOW()::text)
            ),
            updated_at = NOW()
        WHERE thread_id = $1
          AND message_type = 'task_subscription_notification'
          AND COALESCE(metadata->>'consumed_at', '') = ''
        "#,
        thread_id,
    )
    .execute(executor)
    .await?;
    Ok(res.rows_affected())
}
