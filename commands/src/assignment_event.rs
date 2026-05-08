use common::{HasDbRouter, ReadConsistency, error::AppError};
use models::{ProjectTaskBoardItem, ProjectTaskBoardItemAssignment};
use queries::ListProjectTaskBoardItemAssignmentsQuery;
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
    pub event_log_id: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub preempt_summary: &'a str,
}

pub struct BoardItemEditOutcome {
    pub item: ProjectTaskBoardItem,
    pub changed_fields: Vec<TaskRoutingFieldChange>,
    pub preempted: bool,
    pub routed: bool,
}

impl<'a> ApplyBoardItemEditCommand<'a> {
    pub async fn execute(self, pool: &sqlx::PgPool) -> Result<BoardItemEditOutcome, AppError> {
        let original = sqlx::query_as!(
            ProjectTaskBoardItem,
            r#"
            SELECT id, board_id, task_key, title, description, status,
                   assigned_thread_id, metadata, completed_at, archived_at,
                   created_at, updated_at, state_version,
                   schedule_id, scheduled_for, fired_at,
                   pending_question, pending_approval, mounts
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
                      pending_question, pending_approval, mounts
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
            InsertTaskRoutingEvent {
                event_log_id: self.event_log_id,
                deployment_id: self.deployment_id,
                coordinator_thread_id,
                board_item: &item,
                idempotency_key: format!(
                    "task_routing_{}_{}_{}",
                    item.id, item.state_version, self.event_log_id
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

        tx.commit().await?;

        Ok(BoardItemEditOutcome {
            item,
            changed_fields,
            preempted,
            routed,
        })
    }
}
