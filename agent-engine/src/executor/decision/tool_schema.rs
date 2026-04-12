use super::core::AgentExecutor;
use dto::json::ProjectTaskBoardPromptItem;
use models::{AiTool, AiToolConfiguration, SchemaField};
use serde_json::{json, Value};

impl AgentExecutor {
    pub(crate) fn build_flat_tool_selection_properties(
        &self,
        available_tools: &[AiTool],
        active_board_item: Option<&ProjectTaskBoardPromptItem>,
    ) -> Value {
        let mut properties = serde_json::Map::new();

        for tool in available_tools {
            properties.insert(
                tool.name.clone(),
                self.flat_tool_selection_object_schema(tool, active_board_item),
            );
        }

        Value::Object(properties)
    }

    pub(super) fn flat_tool_selection_object_schema(
        &self,
        tool: &AiTool,
        active_board_item: Option<&ProjectTaskBoardPromptItem>,
    ) -> Value {
        let mut fields = match &tool.configuration {
            AiToolConfiguration::Internal(config) => {
                config.input_schema.clone().unwrap_or_default()
            }
            AiToolConfiguration::UseExternalService(config) => {
                config.input_schema.clone().unwrap_or_default()
            }
            AiToolConfiguration::CodeRunner(config) => {
                config.input_schema.clone().unwrap_or_default()
            }
            AiToolConfiguration::Api(config) => {
                config.request_body_schema.clone().unwrap_or_default()
            }
            AiToolConfiguration::PlatformEvent(_) => Vec::new(),
        };
        self.constrain_tool_input_schema(tool, &mut fields, active_board_item);

        let mut properties = serde_json::Map::new();
        properties.insert(
            "selected".to_string(),
            json!({
                "type": "boolean",
                "description": format!("Set true when selecting the {} tool. Omit the tool object entirely when the tool is not selected.", tool.name),
            }),
        );
        let filtered_fields = fields
            .into_iter()
            .filter(|field| !matches!(field.name.as_str(), "selected" | "input"))
            .collect::<Vec<_>>();
        let input_schema = SchemaField::object_json_schema(&filtered_fields);

        properties.insert(
            "input".to_string(),
            json!({
                "type": "array",
                "minItems": 1,
                "description": "One or more exact input objects for this selected tool. Each array item becomes a separate tool call.",
                "items": input_schema
            }),
        );

        json!({
            "type": "object",
            "description": tool.description.clone().unwrap_or_else(|| format!("Selection object for the {} tool.", tool.name)),
            "properties": properties,
            "required": ["selected", "input"]
        })
    }

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
