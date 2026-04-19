use super::core::AgentExecutor;
use dto::json::{
    ProjectTaskBoardAssignmentPromptItem, ProjectTaskBoardItemEventPromptItem,
    ProjectTaskBoardPromptItem, ThreadEventPromptItem, ThreadEventPromptPayload,
};
use models::{ProjectTaskBoardItem, ThreadEvent};

impl AgentExecutor {
    pub(crate) fn project_task_board_item_to_prompt_item(
        item: &ProjectTaskBoardItem,
    ) -> ProjectTaskBoardPromptItem {
        Self::project_task_board_item_to_prompt_item_with_relations(item, None, Vec::new())
    }

    pub(super) fn project_task_board_item_to_prompt_item_with_relations(
        item: &ProjectTaskBoardItem,
        parent_task_key: Option<String>,
        child_task_keys: Vec<String>,
    ) -> ProjectTaskBoardPromptItem {
        ProjectTaskBoardPromptItem {
            task_key: item.task_key.clone(),
            title: item.title.clone(),
            description: item.description.clone(),
            status: item.status.clone(),
            priority: item.priority.clone(),
            assigned_thread_id: item.assigned_thread_id,
            parent_task_key,
            child_task_keys,
            metadata: item.typed_metadata(),
            completed_at: item.completed_at.map(|dt| dt.to_rfc3339()),
            updated_at: item.updated_at.to_rfc3339(),
        }
    }

    pub(crate) fn assignment_prompt_item_from_row(
        assignment: &models::ProjectTaskBoardItemAssignment,
    ) -> ProjectTaskBoardAssignmentPromptItem {
        let metadata = assignment.typed_metadata();
        ProjectTaskBoardAssignmentPromptItem {
            mode: None,
            assignment_id: assignment.id,
            board_item_id: assignment.board_item_id,
            thread_id: assignment.thread_id,
            assignment_role: assignment.assignment_role.clone(),
            assignment_order: assignment.assignment_order,
            status: assignment.status.clone(),
            note: None,
            instructions: assignment.instructions.clone(),
            handoff_file_path: assignment.handoff_file_path.clone(),
            requested_target: metadata.requested_target,
            result_status: assignment.result_status.clone(),
            result_summary: assignment.result_summary.clone(),
        }
    }

    pub(crate) fn board_item_event_prompt_item(
        event: models::ProjectTaskBoardItemEvent,
    ) -> ProjectTaskBoardItemEventPromptItem {
        let assignment_details = event.assignment_event_details();
        let raw_details_json = if assignment_details.is_none()
            && !event.details.is_null()
            && event.details != serde_json::json!({})
        {
            serde_json::to_string(&event.details).ok()
        } else {
            None
        };

        ProjectTaskBoardItemEventPromptItem {
            event_type: event.event_type,
            summary: event.summary,
            body_markdown: event.body_markdown,
            thread_id: event.thread_id,
            execution_run_id: event.execution_run_id,
            raw_details_json,
            assignment_details,
            created_at: event.created_at.to_rfc3339(),
        }
    }

    pub(crate) fn thread_event_prompt_item(event: &ThreadEvent) -> ThreadEventPromptItem {
        let payload = if let Some(payload) = event.conversation_payload() {
            ThreadEventPromptPayload::Conversation { payload }
        } else if let Some(payload) = event.approval_response_received_payload() {
            ThreadEventPromptPayload::ApprovalResponseReceived { payload }
        } else if let Some(payload) = event.task_routing_payload() {
            ThreadEventPromptPayload::TaskRouting { payload }
        } else {
            ThreadEventPromptPayload::Raw {
                raw_json: serde_json::to_string(&event.payload)
                    .unwrap_or_else(|_| "{}".to_string()),
            }
        };

        ThreadEventPromptItem {
            event_id: event.id,
            event_type: event.event_type.clone(),
            board_item_id: event.board_item_id,
            caused_by_thread_id: event.caused_by_thread_id,
            payload,
        }
    }
}
