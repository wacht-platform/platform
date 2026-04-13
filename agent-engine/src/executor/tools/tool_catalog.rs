use super::core::AgentExecutor;
use super::tool_params::MAX_LOADED_EXTERNAL_TOOLS;
use crate::llm::{
    NativeToolDefinition, SemanticLlmMessage, SemanticLlmPromptConfig, SemanticLlmRequest,
};
use common::error::AppError;
use dto::json::agent_executor::{
    ExternalToolCall, LoadToolsParams, SearchToolsParams, ToolCallRequest,
};
use models::{AiTool, AiToolConfiguration, SchemaField};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
struct ToolSearchResponse {
    results: Vec<ToolSearchQueryResult>,
    #[serde(default)]
    recommended_tool_names: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ToolSearchQueryResult {
    query: String,
    #[serde(default)]
    matches: Vec<ToolSearchMatch>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ToolSearchMatch {
    tool_name: String,
    reason: String,
    #[serde(default)]
    recommended: bool,
}

impl AgentExecutor {
    fn tool_schema_for_execution(
        &self,
        tool: &AiTool,
        active_board_item: Option<&dto::json::ProjectTaskBoardPromptItem>,
    ) -> Vec<SchemaField> {
        let mut fields = Self::tool_input_schema_for_search(tool);
        self.constrain_tool_input_schema(tool, &mut fields, active_board_item);
        fields
    }

    fn normalize_tool_input_value(input: Value, schema_fields: &[SchemaField]) -> Value {
        match input {
            Value::Object(map) => {
                Value::Object(Self::normalize_tool_input_object(map, schema_fields))
            }
            other => other,
        }
    }

    fn normalize_tool_input_object(
        mut input: serde_json::Map<String, Value>,
        schema_fields: &[SchemaField],
    ) -> serde_json::Map<String, Value> {
        for field in schema_fields {
            let Some(current_value) = input.get(&field.name).cloned() else {
                continue;
            };

            if current_value.is_null() && !field.required {
                match field.field_type.as_str() {
                    "ARRAY" => {
                        input.insert(field.name.clone(), Value::Array(Vec::new()));
                    }
                    "OBJECT" => {
                        let nested = field.properties.as_deref().unwrap_or(&[]);
                        input.insert(
                            field.name.clone(),
                            Value::Object(Self::normalize_tool_input_object(
                                serde_json::Map::new(),
                                nested,
                            )),
                        );
                    }
                    _ => {
                        input.remove(&field.name);
                    }
                }
                continue;
            }

            let normalized_value = match current_value {
                Value::Object(nested) if field.field_type == "OBJECT" => {
                    let nested_schema = field.properties.as_deref().unwrap_or(&[]);
                    Value::Object(Self::normalize_tool_input_object(nested, nested_schema))
                }
                Value::Array(items) if field.field_type == "ARRAY" => {
                    let normalized_items = if field.items_type.as_deref() == Some("OBJECT") {
                        let nested_schema = field
                            .items_schema
                            .as_deref()
                            .and_then(|item| item.properties.as_deref())
                            .unwrap_or(&[]);
                        items
                            .into_iter()
                            .map(|item| match item {
                                Value::Object(nested) => Value::Object(
                                    Self::normalize_tool_input_object(nested, nested_schema),
                                ),
                                other => other,
                            })
                            .collect()
                    } else {
                        items
                    };
                    Value::Array(normalized_items)
                }
                other => other,
            };

            input.insert(field.name.clone(), normalized_value);
        }

        input
    }

    pub(crate) fn tool_input_schema_for_search(tool: &AiTool) -> Vec<SchemaField> {
        match &tool.configuration {
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
        }
    }

    pub(crate) fn validate_selected_tool_input(
        tool: &AiTool,
        input_object: &serde_json::Map<String, Value>,
    ) -> Result<(), AppError> {
        let missing_required_fields = Self::tool_input_schema_for_search(tool)
            .into_iter()
            .filter(|field| field.required)
            .map(|field| field.name)
            .filter(|field_name| !input_object.contains_key(field_name))
            .collect::<Vec<_>>();

        if missing_required_fields.is_empty() {
            return Ok(());
        }

        Err(AppError::BadRequest(format!(
            "Selected tool '{}' is missing required parameters: {}",
            tool.name,
            missing_required_fields.join(", ")
        )))
    }

    pub(crate) fn build_native_tool_definition(
        &self,
        tool: &AiTool,
        active_board_item: Option<&dto::json::ProjectTaskBoardPromptItem>,
    ) -> NativeToolDefinition {
        NativeToolDefinition {
            name: tool.name.clone(),
            description: tool
                .description
                .clone()
                .unwrap_or_else(|| format!("Call the {} tool.", tool.name)),
            input_schema: SchemaField::object_json_schema(
                &self.tool_schema_for_execution(tool, active_board_item),
            ),
        }
    }

    pub(crate) fn build_tool_call_request_from_native_call(
        &self,
        tool: &AiTool,
        input_object: serde_json::Map<String, Value>,
    ) -> Result<ToolCallRequest, AppError> {
        Self::validate_selected_tool_input(tool, &input_object)?;
        Self::build_tool_call_request(tool, Value::Object(input_object))
    }

    pub(crate) fn parse_tool_params<T: DeserializeOwned>(
        tool_name: &str,
        input: Value,
    ) -> Result<T, AppError> {
        serde_json::from_value(input)
            .map_err(|e| AppError::BadRequest(format!("Invalid {tool_name} params: {e}")))
    }

    pub(crate) fn build_tool_call_request(
        tool: &AiTool,
        input: Value,
    ) -> Result<ToolCallRequest, AppError> {
        let input_schema = Self::tool_input_schema_for_search(tool);
        let normalized_input = Self::normalize_tool_input_value(input, &input_schema);

        match &tool.configuration {
            AiToolConfiguration::Internal(config) => match config.tool_type {
                models::InternalToolType::SearchTools => Ok(ToolCallRequest::SearchTools {
                    params: Self::parse_tool_params("search_tools", normalized_input)?,
                }),
                models::InternalToolType::LoadTools => Ok(ToolCallRequest::LoadTools {
                    params: Self::parse_tool_params("load_tools", normalized_input)?,
                }),
                models::InternalToolType::ReadImage => Ok(ToolCallRequest::ReadImage {
                    params: Self::parse_tool_params("read_image", normalized_input)?,
                }),
                models::InternalToolType::ReadFile => Ok(ToolCallRequest::ReadFile {
                    params: Self::parse_tool_params("read_file", normalized_input)?,
                }),
                models::InternalToolType::WriteFile => Ok(ToolCallRequest::WriteFile {
                    params: Self::parse_tool_params("write_file", normalized_input)?,
                }),
                models::InternalToolType::EditFile => Ok(ToolCallRequest::EditFile {
                    params: Self::parse_tool_params("edit_file", normalized_input)?,
                }),
                models::InternalToolType::ExecuteCommand => Ok(ToolCallRequest::ExecuteCommand {
                    params: Self::parse_tool_params("execute_command", normalized_input)?,
                }),
                models::InternalToolType::Sleep => Ok(ToolCallRequest::Sleep {
                    params: Self::parse_tool_params("sleep", normalized_input)?,
                }),
                models::InternalToolType::SnapshotExecutionState => {
                    Ok(ToolCallRequest::SnapshotExecutionState {
                        params: Self::parse_tool_params(
                            "snapshot_execution_state",
                            normalized_input,
                        )?,
                    })
                }
                models::InternalToolType::WebSearch => Ok(ToolCallRequest::WebSearch {
                    params: Self::parse_tool_params("web_search", normalized_input)?,
                }),
                models::InternalToolType::UrlContent => Ok(ToolCallRequest::UrlContent {
                    params: Self::parse_tool_params("url_content", normalized_input)?,
                }),
                models::InternalToolType::SearchKnowledgebase => {
                    Ok(ToolCallRequest::SearchKnowledgebase {
                        params: Self::parse_tool_params("search_knowledgebase", normalized_input)?,
                    })
                }
                models::InternalToolType::LoadMemory => Ok(ToolCallRequest::LoadMemory {
                    params: Self::parse_tool_params("load_memory", normalized_input)?,
                }),
                models::InternalToolType::SaveMemory => Ok(ToolCallRequest::SaveMemory {
                    params: Self::parse_tool_params("save_memory", normalized_input)?,
                }),
                models::InternalToolType::CreateProjectTask => {
                    Ok(ToolCallRequest::CreateProjectTask {
                        params: Self::parse_tool_params("create_project_task", normalized_input)?,
                    })
                }
                models::InternalToolType::UpdateProjectTask => {
                    Ok(ToolCallRequest::UpdateProjectTask {
                        params: Self::parse_tool_params("update_project_task", normalized_input)?,
                    })
                }
                models::InternalToolType::AssignProjectTask => {
                    Ok(ToolCallRequest::AssignProjectTask {
                        params: Self::parse_tool_params("assign_project_task", normalized_input)?,
                    })
                }
                models::InternalToolType::ListThreads => Ok(ToolCallRequest::ListThreads {
                    params: Self::parse_tool_params("list_threads", normalized_input)?,
                }),
                models::InternalToolType::CreateThread => Ok(ToolCallRequest::CreateThread {
                    params: Self::parse_tool_params("create_thread", normalized_input)?,
                }),
                models::InternalToolType::UpdateThread => Ok(ToolCallRequest::UpdateThread {
                    params: Self::parse_tool_params("update_thread", normalized_input)?,
                }),
                models::InternalToolType::TaskGraphAddNode => {
                    Ok(ToolCallRequest::TaskGraphAddNode {
                        params: Self::parse_tool_params("task_graph_add_node", normalized_input)?,
                    })
                }
                models::InternalToolType::TaskGraphAddDependency => {
                    Ok(ToolCallRequest::TaskGraphAddDependency {
                        params: Self::parse_tool_params(
                            "task_graph_add_dependency",
                            normalized_input,
                        )?,
                    })
                }
                models::InternalToolType::TaskGraphMarkInProgress => {
                    Ok(ToolCallRequest::TaskGraphMarkInProgress {
                        params: Self::parse_tool_params(
                            "task_graph_mark_in_progress",
                            normalized_input,
                        )?,
                    })
                }
                models::InternalToolType::TaskGraphCompleteNode => {
                    Ok(ToolCallRequest::TaskGraphCompleteNode {
                        params: Self::parse_tool_params(
                            "task_graph_complete_node",
                            normalized_input,
                        )?,
                    })
                }
                models::InternalToolType::TaskGraphFailNode => {
                    Ok(ToolCallRequest::TaskGraphFailNode {
                        params: Self::parse_tool_params("task_graph_fail_node", normalized_input)?,
                    })
                }
                models::InternalToolType::TaskGraphMarkCompleted => {
                    Ok(ToolCallRequest::TaskGraphMarkCompleted {
                        params: Self::parse_tool_params(
                            "task_graph_mark_completed",
                            normalized_input,
                        )?,
                    })
                }
                models::InternalToolType::TaskGraphMarkFailed => {
                    Ok(ToolCallRequest::TaskGraphMarkFailed {
                        params: Self::parse_tool_params(
                            "task_graph_mark_failed",
                            normalized_input,
                        )?,
                    })
                }
                models::InternalToolType::AppendTaskJournal => Err(AppError::BadRequest(
                    "append_task_journal is not exposed in the runtime".to_string(),
                )),
            },
            _ => Ok(ToolCallRequest::External(ExternalToolCall {
                tool_name: tool.name.clone(),
                input: normalized_input,
            })),
        }
    }

    pub(crate) fn external_tool_catalog(&self) -> Vec<AiTool> {
        self.ctx
            .agent
            .tools
            .iter()
            .filter(|tool| !matches!(tool.tool_type, models::AiToolType::Internal))
            .cloned()
            .collect()
    }

    pub(crate) async fn execute_search_tools(
        &self,
        params: SearchToolsParams,
    ) -> Result<Value, AppError> {
        let queries = params
            .queries
            .into_iter()
            .map(|query| query.trim().to_string())
            .filter(|query| !query.is_empty())
            .take(10)
            .collect::<Vec<_>>();
        if queries.is_empty() {
            return Ok(json!({
                "results": [],
                "recommended_tool_names": [],
                "tool_catalog_size": 0
            }));
        }

        let max_results_per_query = params.max_results_per_query.unwrap_or(3).clamp(1, 5);
        let external_tools = self.external_tool_catalog();
        if external_tools.is_empty() {
            return Ok(json!({
                "results": [],
                "recommended_tool_names": [],
                "tool_catalog_size": 0
            }));
        }

        let tool_names = external_tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>();
        let tool_catalog = external_tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": Self::tool_input_schema_for_search(tool),
                })
            })
            .collect::<Vec<_>>();

        let response_schema = json!({
            "type": "OBJECT",
            "properties": {
                "results": {
                    "type": "ARRAY",
                    "items": {
                        "type": "OBJECT",
                        "properties": {
                            "query": { "type": "STRING" },
                            "matches": {
                                "type": "ARRAY",
                                "items": {
                                    "type": "OBJECT",
                                    "properties": {
                                        "tool_name": { "type": "STRING", "enum": tool_names },
                                        "reason": { "type": "STRING" },
                                        "recommended": { "type": "BOOLEAN" }
                                    },
                                    "required": ["tool_name", "reason", "recommended"]
                                }
                            }
                        },
                        "required": ["query", "matches"]
                    }
                },
                "recommended_tool_names": {
                    "type": "ARRAY",
                    "items": { "type": "STRING", "enum": tool_names }
                }
            },
            "required": ["results", "recommended_tool_names"]
        });

        let prompt = format!(
            r#"Find the best external tool matches for the given queries.
Use only exact tool names from the provided catalog.
Consider the chat history and current user request as context.
Return at most {max_results_per_query} matches per query.

Current user request:
{}

Queries:
{}

External tool catalog with descriptions and input schemas:
{}"#,
            self.user_request,
            serde_json::to_string_pretty(&queries).unwrap_or_else(|_| "[]".to_string()),
            serde_json::to_string_pretty(&tool_catalog).unwrap_or_else(|_| "[]".to_string())
        );

        let config = SemanticLlmPromptConfig {
            response_json_schema: response_schema,
            temperature: None,
            max_output_tokens: None,
            reasoning_effort: None,
        };
        let messages = self
            .get_conversation_history_for_llm()
            .await
            .iter()
            .map(Self::semantic_message_from_history_entry)
            .chain(std::iter::once(SemanticLlmMessage::text("user", prompt)))
            .collect::<Vec<_>>();
        let request = SemanticLlmRequest::from_config(
            r#"You are a tool matcher. Match the request to the best external tools from the provided catalog. Use only exact tool names from the catalog and be conservative."#
                .to_string(),
            messages,
            config,
        );

        let response = self
            .create_weak_llm()
            .await?
            .generate_structured_from_prompt::<ToolSearchResponse>(request, None)
            .await?;
        let response = response.value;

        let catalog_by_name = external_tools
            .iter()
            .map(|tool| (tool.name.clone(), tool))
            .collect::<HashMap<_, _>>();

        let results = response
            .results
            .into_iter()
            .map(|result| {
                let matches = result
                    .matches
                    .into_iter()
                    .filter_map(|matched| {
                        let tool = catalog_by_name.get(&matched.tool_name)?;
                        Some(json!({
                            "tool_name": matched.tool_name,
                            "reason": matched.reason,
                            "recommended": matched.recommended,
                            "description": tool.description,
                            "input_schema": Self::tool_input_schema_for_search(tool),
                        }))
                    })
                    .collect::<Vec<_>>();
                json!({
                    "query": result.query,
                    "matches": matches,
                })
            })
            .collect::<Vec<_>>();

        Ok(json!({
            "results": results,
            "recommended_tool_names": response.recommended_tool_names,
            "tool_catalog_size": external_tools.len(),
        }))
    }

    pub(crate) async fn execute_load_tools(
        &mut self,
        params: LoadToolsParams,
    ) -> Result<Value, AppError> {
        let requested_tool_names = params
            .tool_names
            .into_iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        if requested_tool_names.is_empty() {
            return Ok(json!({
                "loaded_tool_names": [],
                "evicted_tool_names": [],
                "not_found_tool_names": [],
                "currently_loaded_tool_names": [],
            }));
        }

        let external_tools = self.external_tool_catalog();
        let external_by_name = external_tools
            .iter()
            .map(|tool| (tool.name.clone(), tool))
            .collect::<HashMap<_, _>>();

        let mut matched_tools = Vec::new();
        let mut not_found_tool_names = Vec::new();
        for tool_name in requested_tool_names {
            if let Some(tool) = external_by_name.get(&tool_name) {
                matched_tools.push((*tool).clone());
            } else {
                not_found_tool_names.push(tool_name);
            }
        }

        for tool in &matched_tools {
            self.loaded_external_tool_ids
                .retain(|tool_id| *tool_id != tool.id);
            self.loaded_external_tool_ids.push(tool.id);
        }

        let mut evicted_tool_names = Vec::new();
        while self.loaded_external_tool_ids.len() > MAX_LOADED_EXTERNAL_TOOLS {
            let evicted_id = self.loaded_external_tool_ids.remove(0);
            if let Some(tool) = external_tools.iter().find(|tool| tool.id == evicted_id) {
                evicted_tool_names.push(tool.name.clone());
            }
        }

        let currently_loaded_tools = self
            .loaded_external_tool_ids
            .iter()
            .filter_map(|tool_id| external_tools.iter().find(|tool| tool.id == *tool_id))
            .cloned()
            .collect::<Vec<_>>();

        Ok(json!({
            "loaded_tool_names": matched_tools.iter().map(|tool| tool.name.clone()).collect::<Vec<_>>(),
            "evicted_tool_names": evicted_tool_names,
            "not_found_tool_names": not_found_tool_names,
            "currently_loaded_tool_names": currently_loaded_tools.iter().map(|tool| tool.name.clone()).collect::<Vec<_>>(),
            "currently_loaded_tools": currently_loaded_tools.iter().map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": Self::tool_input_schema_for_search(tool),
                })
            }).collect::<Vec<_>>(),
        }))
    }
}
