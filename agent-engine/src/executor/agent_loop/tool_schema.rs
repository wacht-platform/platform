use super::core::AgentExecutor;
use dto::json::ProjectTaskBoardPromptItem;
use models::{AiTool, SchemaField};
use serde_json::json;

impl AgentExecutor {
    pub(crate) fn constrain_tool_input_schema(
        &self,
        tool: &AiTool,
        fields: &mut [SchemaField],
        active_board_item: Option<&ProjectTaskBoardPromptItem>,
    ) {
        if !self.effective_is_coordinator_thread() {
            return;
        }

        if matches!(
            tool.name.as_str(),
            "assign_project_task" | "update_project_task"
        ) {
            if let Some(active_board_item) = active_board_item {
                if let Some(task_key_field) =
                    fields.iter_mut().find(|field| field.name == "task_key")
                {
                    task_key_field.enum_values =
                        Some(vec![json!(active_board_item.task_key.clone())]);
                    task_key_field.description = Some(format!(
                        "Must be the active task key `{}` for this coordinator decision.",
                        active_board_item.task_key
                    ));
                }
            }
        }

        if tool.name == "update_project_task" {
            if let Some(status_field) = fields.iter_mut().find(|field| field.name == "status") {
                status_field.enum_values = Some(vec![
                    json!("pending"),
                    json!("blocked"),
                    json!("completed"),
                    json!("failed"),
                ]);
                status_field.description = Some(
                    "Optional updated task status for coordinator decisions. Omit when status should stay unchanged. Do not use `in_progress` from the coordinator lane; the assigned execution lane owns that transition.".to_string(),
                );
            }
        }
    }
}
