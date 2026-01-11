use super::core::AgentExecutor;
use crate::template::{render_template_with_prompt, AgentTemplates};

use commands::Command;
use common::error::AppError;
use dto::json::agent_responses::{ExecutionAction, ParameterGenerationResponse, TaskType};
use dto::json::{ToolCall, WorkflowCall};
use models::{
    AiTool, AiToolConfiguration, ApiToolConfiguration, PlatformFunctionToolConfiguration,
    SchemaField,
};
use queries::Query;
use serde_json::{json, Value};
use tracing::{info, warn};

impl AgentExecutor {
    pub(super) async fn execute_action(&self, action: &ExecutionAction) -> Result<Value, AppError> {
        info!(
            action_type = ?action.action_type,
            purpose = %action.purpose,
            "Executing action"
        );

        let result = match &action.action_type {
            TaskType::ToolCall => {
                let tool_call = self.parse_tool_call(&action.details, &action.purpose, action.context_messages).await?;
                info!(
                    tool_name = %tool_call.tool_name,
                    parameters = %tool_call.parameters,
                    "Parsed tool call"
                );
                let tool = self
                    .agent
                    .tools
                    .iter()
                    .find(|t| t.name == tool_call.tool_name)
                    .ok_or_else(|| {
                        warn!(tool_name = %tool_call.tool_name, "Tool not found");
                        AppError::BadRequest(format!("Tool '{}' not found", tool_call.tool_name))
                    })?;

                let title = &self.context_title;
                self.tool_executor
                    .execute_tool_immediately(tool, tool_call.parameters, &self.filesystem, &self.shell, title)
                    .await
            }
            TaskType::WorkflowCall => {
                let workflow_call = self.parse_workflow_call(&action.details)?;

                let conversation_context: Vec<Value> = self
                    .conversations
                    .iter()
                    .map(|conv| {
                        json!({
                            "id": conv.id,
                            "message_type": conv.message_type,
                            "content": conv.content,
                            "timestamp": conv.timestamp,
                            "type": "conversation"
                        })
                    })
                    .collect();

                let memory_context: Vec<Value> = self
                    .memories
                    .iter()
                    .map(|mem| {
                        json!({
                            "id": mem.id,
                            "content": mem.content,
                            "category": mem.memory_category,
                            "temporal_score": mem.base_temporal_score,
                            "access_count": mem.access_count,
                            "timestamp": mem.last_accessed_at,
                            "type": "memory"
                        })
                    })
                    .collect();

                self.execute_workflow_task(
                    &workflow_call,
                    &self.agent.workflows,
                    &conversation_context,
                    &memory_context,
                    self.channel.clone(),
                )
                .await
            }
        };

        if let Some(ref actionable_id) = action.clear_actionable_id {
            if let Ok(current_context) = queries::GetExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
                .execute(&self.app_state)
                .await
            {
                if let Some(mut metadata) = current_context.external_resource_metadata {
                    if let Some(actionables) = metadata.get_mut("actionables") {
                        if let Some(arr) = actionables.as_array_mut() {
                            let original_len = arr.len();
                            arr.retain(|a| a.get("id").and_then(|id| id.as_str()) != Some(actionable_id.as_str()));
                            
                            if arr.len() < original_len {
                                let _ = commands::UpdateExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
                                    .with_external_resource_metadata(metadata)
                                    .execute(&self.app_state)
                                    .await;
                                
                                tracing::info!(
                                    context_id = self.context_id,
                                    actionable_id = %actionable_id,
                                    "Cleared actionable from context after tool execution"
                                );
                            }
                        }
                    }
                }
            }
        }

        result
    }

    fn schema_fields_to_properties(fields: &[SchemaField]) -> (Value, Vec<String>) {
        let mut properties = serde_json::Map::new();
        let mut required_fields = Vec::new();

        for field in fields {
            let mut field_def = serde_json::Map::new();
            let field_type_lower = field.field_type.to_lowercase();
            field_def.insert("type".to_string(), json!(field_type_lower));

            if let Some(ref items_type) = field.items_type {
                field_def.insert("items".to_string(), json!({
                    "type": items_type.to_lowercase()
                }));
            }

            if let Some(desc) = &field.description {
                field_def.insert("description".to_string(), json!(desc));
            }

            if field.required {
                required_fields.push(field.name.clone());
            }

            properties.insert(field.name.clone(), json!(field_def));
        }

        (json!(properties), required_fields)
    }

    fn organize_api_parameters(
        &self,
        flat_params: Value,
        api_config: &ApiToolConfiguration,
    ) -> Result<Value, AppError> {
        let params_obj = flat_params.as_object().ok_or_else(|| {
            AppError::Internal("Generated parameters must be an object".to_string())
        })?;

        let mut url_params = serde_json::Map::new();
        let mut body_params = serde_json::Map::new();

        let field_in_schema = |field_name: &str, schema: &Option<Vec<SchemaField>>| {
            schema
                .as_ref()
                .is_some_and(|fields| fields.iter().any(|f| f.name == field_name))
        };

        for (key, value) in params_obj {
            if field_in_schema(key, &api_config.url_params_schema) {
                url_params.insert(key.clone(), value.clone());
            } else if field_in_schema(key, &api_config.request_body_schema) {
                body_params.insert(key.clone(), value.clone());
            }
        }

        let mut result = serde_json::Map::new();

        if !url_params.is_empty() {
            result.insert("url_params".to_string(), json!(url_params));
        }

        if !body_params.is_empty() {
            result.insert("body".to_string(), json!(body_params));
        }

        Ok(json!(result))
    }

    async fn parse_tool_call(&self, details: &Value, action_purpose: &str, context_messages: u32) -> Result<ToolCall, AppError> {
        let tool_name = details["tool_name"]
            .as_str()
            .ok_or_else(|| AppError::BadRequest("Tool name not specified".to_string()))?;

        let tool = self.find_tool(tool_name)?;
        let params = self.get_tool_parameters(tool, details, action_purpose, context_messages).await?;

        Ok(ToolCall {
            tool_name: tool_name.to_string(),
            parameters: params,
        })
    }

    fn find_tool(&self, tool_name: &str) -> Result<&AiTool, AppError> {
        self.agent
            .tools
            .iter()
            .find(|t| t.name == tool_name)
            .ok_or_else(|| AppError::BadRequest(format!("Tool '{tool_name}' not found")))
    }

    async fn get_tool_parameters(&self, tool: &AiTool, details: &Value, action_purpose: &str, context_messages: u32) -> Result<Value, AppError> {
        if self.tool_needs_llm_params(tool) {
            let generated_params = self.generate_tool_parameters(tool, action_purpose, context_messages).await?;
            return match &tool.configuration {
                AiToolConfiguration::Api(api_config) => {
                    self.organize_api_parameters(generated_params, api_config)
                }
                _ => Ok(generated_params),
            };
        }

        Ok(self.get_default_tool_parameters(tool, details))
    }

    fn tool_needs_llm_params(&self, tool: &AiTool) -> bool {
        match &tool.configuration {
            AiToolConfiguration::Api(api_config) => {
                api_config.request_body_schema.is_some() || api_config.url_params_schema.is_some()
            }
            AiToolConfiguration::PlatformFunction(func_config) => {
                func_config.input_schema.is_some()
            }
            AiToolConfiguration::Internal(internal_config) => {
                internal_config.input_schema.as_ref().is_some_and(|s| !s.is_empty())
            }
            AiToolConfiguration::UseExternalService(external_config) => {
                external_config.input_schema.as_ref().is_some_and(|s| !s.is_empty())
            }
            _ => false,
        }
    }

    fn get_default_tool_parameters(&self, tool: &AiTool, details: &Value) -> Value {
        match &tool.configuration {
            AiToolConfiguration::KnowledgeBase(_) => {
                json!({
                    "query": details.get("query")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&self.user_request)
                })
            }
            AiToolConfiguration::PlatformEvent(event_config) => {
                event_config.event_data.clone().unwrap_or(json!({}))
            }
            _ => json!({}),
        }
    }

    async fn generate_tool_parameters(&self, tool: &AiTool, action_purpose: &str, context_messages: u32) -> Result<Value, AppError> {
        let parameter_schema = self.build_parameter_schema(tool)?;

        if parameter_schema == json!({}) {
            return Ok(json!({}));
        }

        let response = self
            .request_parameter_generation(tool, &parameter_schema, action_purpose, context_messages)
            .await?;

        if !response.parameter_generation.can_generate {
            return Err(AppError::BadRequest(format!(
                "Cannot generate parameters for {}: Missing information - {}",
                tool.name,
                response.parameter_generation.missing_information.join(", ")
            )));
        }

        Ok(response.parameter_generation.parameters)
    }

    fn build_parameter_schema(&self, tool: &AiTool) -> Result<Value, AppError> {
        match &tool.configuration {
            AiToolConfiguration::Api(api_config) => self.build_api_schema(api_config),
            AiToolConfiguration::PlatformFunction(func_config) => {
                self.build_platform_function_schema(func_config)
            }
            AiToolConfiguration::Internal(internal_config) => {
                self.build_internal_schema(internal_config)
            }
            AiToolConfiguration::UseExternalService(external_config) => {
                self.build_external_service_schema(external_config)
            }
            _ => Err(AppError::Internal(
                "Parameter generation not supported for this tool type".to_string(),
            )),
        }
    }

    fn build_api_schema(&self, api_config: &ApiToolConfiguration) -> Result<Value, AppError> {
        let mut all_properties = serde_json::Map::new();
        let mut all_required = Vec::new();

        if let Some(schema) = &api_config.request_body_schema {
            let (properties, required) = Self::schema_fields_to_properties(schema);
            if let Some(props) = properties.as_object() {
                all_properties.extend(props.clone());
            }
            all_required.extend(required);
        }

        if let Some(schema) = &api_config.url_params_schema {
            let (properties, required) = Self::schema_fields_to_properties(schema);
            if let Some(props) = properties.as_object() {
                all_properties.extend(props.clone());
            }
            all_required.extend(required);
        }

        if all_properties.is_empty() {
            return Ok(json!({}));
        }

        Ok(json!({
            "type": "OBJECT",
            "properties": all_properties,
            "required": all_required
        }))
    }

    fn build_platform_function_schema(
        &self,
        func_config: &PlatformFunctionToolConfiguration,
    ) -> Result<Value, AppError> {
        let schema = func_config
            .input_schema
            .as_ref()
            .ok_or_else(|| AppError::Internal("No input schema".to_string()))?;

        let (properties, required) = Self::schema_fields_to_properties(schema);

        if properties.as_object().is_none_or(|p| p.is_empty()) {
            return Ok(json!({}));
        }

        Ok(json!({
            "type": "OBJECT",
            "properties": properties,
            "required": required
        }))
    }

    fn build_internal_schema(
        &self,
        internal_config: &models::InternalToolConfiguration,
    ) -> Result<Value, AppError> {
        let schema = match &internal_config.input_schema {
            Some(s) if !s.is_empty() => s,
            _ => return Ok(json!({})),
        };

        let (properties, required) = Self::schema_fields_to_properties(schema);

        if properties.as_object().is_none_or(|p| p.is_empty()) {
            return Ok(json!({}));
        }

        Ok(json!({
            "type": "OBJECT",
            "properties": properties,
            "required": required
        }))
    }

    fn build_external_service_schema(
        &self,
        external_config: &models::UseExternalServiceToolConfiguration,
    ) -> Result<Value, AppError> {
        let schema = match &external_config.input_schema {
            Some(s) if !s.is_empty() => s,
            _ => return Ok(json!({})),
        };

        let (properties, required) = Self::schema_fields_to_properties(schema);

        if properties.as_object().is_none_or(|p| p.is_empty()) {
            return Ok(json!({}));
        }

        Ok(json!({
            "type": "OBJECT",
            "properties": properties,
            "required": required
        }))
    }

    async fn request_parameter_generation(
        &self,
        tool: &AiTool,
        parameter_schema: &Value,
        action_purpose: &str,
        context_messages: u32,
    ) -> Result<ParameterGenerationResponse, AppError> {
        // Get FILTERED conversation history based on context_messages
        let full_history = self.get_conversation_history_for_llm().await;
        let limited_history: Vec<Value> = if context_messages > 0 && (context_messages as usize) < full_history.len() {
            full_history.into_iter().rev().take(context_messages as usize).rev().collect()
        } else {
            full_history
        };
        
        let mut context_json = json!({
            "conversation_history": limited_history,
            "tool_name": tool.name,
            "tool_description": tool.description.as_ref().unwrap_or(&"".to_string()),
            "parameter_schema": parameter_schema,
            "user_request": self.user_request,
            "current_objective": self.current_objective,
            "conversation_insights": self.conversation_insights,
            "action_purpose": action_purpose,
        });

        if let Some(ref sys_instructions) = self.system_instructions {
            if let Some(obj) = context_json.as_object_mut() {
                let custom_instructions =
                    format!("CUSTOM INSTRUCTIONS FOR THIS CHAT:\n{}\n\n\n Make sure you keep these guidelines in mind but always give more weightage to the previous instructions given to you", sys_instructions);
                obj.insert(
                    "custom_system_instructions".to_string(),
                    json!(custom_instructions),
                );
            }
        }

        let request_body =
            render_template_with_prompt(AgentTemplates::PARAMETER_GENERATION, context_json)
                .map_err(|e| {
                    AppError::Internal(format!(
                        "Failed to render parameter generation template: {e}"
                    ))
                })?;


        let (response, _) = self.create_weak_llm()?
            .generate_structured_content::<ParameterGenerationResponse>(request_body)
            .await?;

        Ok(response)
    }

    pub(super) fn parse_workflow_call(&self, details: &Value) -> Result<WorkflowCall, AppError> {
        let workflow_name = details["workflow_name"]
            .as_str()
            .ok_or_else(|| AppError::BadRequest("Workflow name not specified".to_string()))?;

        let inputs = details.get("inputs").cloned().unwrap_or(json!({}));

        Ok(WorkflowCall {
            workflow_name: workflow_name.to_string(),
            inputs,
        })
    }
}
