use common::{HasDbRouter, HasIdProvider, ReadConsistency, error::AppError};
use models::{
    AssignmentEventKind, ConversationContent, ConversationMessageType, ProjectTaskBoardItem,
    ProjectTaskBoardItemAssignment,
};
use queries::ListProjectTaskBoardItemAssignmentsQuery;

use crate::CreateConversationCommand;

pub struct WriteAssignmentEventConversation {
    pub thread_id: i64,
    pub board_item_id: i64,
    pub kind: AssignmentEventKind,
    pub assignment_id: Option<i64>,
    pub summary: String,
    pub payload: Option<serde_json::Value>,
}

impl WriteAssignmentEventConversation {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<i64, AppError>
    where
        D: HasDbRouter + HasIdProvider + ?Sized,
    {
        let id = deps.id_provider().next_id()? as i64;
        CreateConversationCommand::new(
            id,
            self.thread_id,
            ConversationContent::AssignmentEvent {
                kind: self.kind,
                assignment_id: self.assignment_id,
                thread_event_id: None,
                summary: Some(self.summary),
                payload: self.payload,
            },
            ConversationMessageType::AssignmentEvent,
        )
        .with_board_item_id(self.board_item_id)
        .execute_with_db(deps.writer_pool())
        .await?;
        Ok(id)
    }
}

pub fn build_task_routing_summary(
    board_item: &ProjectTaskBoardItem,
    prior_assignment_count: usize,
) -> String {
    format!(
        "Coordinator received routing signal for task #{} '{}' (status={}, priority={}). {} prior assignment(s) on this task.",
        board_item.id,
        board_item.title,
        board_item.status,
        board_item.priority,
        prior_assignment_count,
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
        "Task #{} '{}' is now active on this thread. Assignment #{} transitioned to in_progress (role={}, order {} of {}). {}.",
        board_item.id,
        board_item.title,
        assignment.id,
        assignment.assignment_role,
        assignment.assignment_order,
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
