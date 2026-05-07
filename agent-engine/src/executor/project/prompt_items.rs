use super::core::AgentExecutor;
use dto::json::{
    BoardItemSchedulePromptInfo, ProjectTaskBoardAssignmentPromptItem, ProjectTaskBoardPromptItem,
    ThreadEventPromptItem, ThreadEventPromptPayload,
};
use models::{ProjectTaskBoardItem, ThreadEvent};
use queries::BoardItemScheduleSummary;

impl AgentExecutor {
    pub(crate) fn project_task_board_item_to_prompt_item(
        item: &ProjectTaskBoardItem,
    ) -> ProjectTaskBoardPromptItem {
        Self::project_task_board_item_to_prompt_item_with_relations(item, None, Vec::new(), None)
    }

    pub(super) fn project_task_board_item_to_prompt_item_with_relations(
        item: &ProjectTaskBoardItem,
        parent_task_key: Option<String>,
        child_task_keys: Vec<String>,
        schedule: Option<&BoardItemScheduleSummary>,
    ) -> ProjectTaskBoardPromptItem {
        ProjectTaskBoardPromptItem {
            task_key: item.task_key.clone(),
            title: item.title.clone(),
            description: item.description.clone(),
            status: item.status.clone(),
            assigned_thread_id: item.assigned_thread_id,
            parent_task_key,
            child_task_keys,
            metadata: item.typed_metadata(),
            completed_at: item.completed_at.map(|dt| dt.to_rfc3339()),
            updated_at: item.updated_at.to_rfc3339(),
            schedule: schedule.map(format_schedule_prompt_info),
        }
    }
}

fn format_schedule_prompt_info(s: &BoardItemScheduleSummary) -> BoardItemSchedulePromptInfo {
    BoardItemSchedulePromptInfo {
        kind: s.kind.clone(),
        interval: s.interval_seconds.map(humanize_interval),
        next_run_at: s.next_run_at.to_rfc3339(),
        last_fired_at: s.last_fired_at.map(|t| t.to_rfc3339()),
        overlap_policy: s.overlap_policy.clone(),
    }
}

pub(crate) fn humanize_interval(seconds: i64) -> String {
    if seconds <= 0 {
        return format!("{seconds}s");
    }
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let secs = seconds % 60;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if secs > 0 && parts.is_empty() {
        parts.push(format!("{secs}s"));
    }
    parts.join(" ")
}

impl AgentExecutor {
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
            status: assignment.status.clone(),
            note: None,
            instructions: assignment.instructions.clone(),
            requested_target: metadata.requested_target,
            result_status: assignment.result_status.clone(),
            result_summary: assignment.result_summary.clone(),
        }
    }

    pub(crate) fn thread_event_prompt_item(event: &ThreadEvent) -> ThreadEventPromptItem {
        let payload = if let Some(payload) = event.task_routing_payload() {
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
