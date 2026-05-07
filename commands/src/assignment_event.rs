use common::{HasDbRouter, ReadConsistency, error::AppError};
use models::{ProjectTaskBoardItem, ProjectTaskBoardItemAssignment};
use queries::ListProjectTaskBoardItemAssignmentsQuery;
use sqlx::Postgres;

use crate::event_log::publish_status;

use crate::event_log;

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
}

impl<'a> InsertTaskRoutingEvent<'a> {
    pub async fn execute<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = Postgres>,
    {
        let payload = serde_json::json!({
            "event_log_id": self.event_log_id.to_string(),
            "deployment_id": self.deployment_id.to_string(),
            "thread_id": self.coordinator_thread_id.to_string(),
            "board_item_id": self.board_item.id.to_string(),
            "kind": "task_routing",
            "routing_reason": self.routing_reason,
            "summary": self.summary,
            "note": self.note,
        });

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
