use super::core::AgentExecutor;
use dto::json::{
    BoardItemMountPromptInfo, BoardItemSchedulePromptInfo, ProjectTaskBoardAssignmentPromptItem,
    ProjectTaskBoardPromptItem, ThreadEventPromptItem, ThreadEventPromptPayload,
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
            description: truncate_prompt_text(item.description.clone(), 1_200),
            status: item.status.clone(),
            mounts: parse_board_item_mounts(&item.mounts),
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

fn parse_board_item_mounts(mounts: &serde_json::Value) -> Vec<BoardItemMountPromptInfo> {
    let Some(arr) = mounts.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|m| {
            let mount_path = m.get("mount_path")?.as_str()?.to_string();
            let mode = m
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("rw")
                .to_string();
            let description = m
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Some(BoardItemMountPromptInfo {
                mount_path,
                mode,
                description,
            })
        })
        .collect()
}

fn truncate_prompt_text(value: Option<String>, max_chars: usize) -> Option<String> {
    let value = value?.trim().to_string();
    if value.chars().count() <= max_chars {
        return Some(value);
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated = truncated.trim_end().to_string();
    truncated.push_str("...");
    Some(truncated)
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
