use super::core::AgentExecutor;
use crate::template::{render_template_with_prompt, AgentTemplates};

use common::error::AppError;
use dto::json::agent_responses::SwitchCaseEvaluation;
use dto::json::{
    CaseDescription, GenerationConfig, LLMContent, LLMGenerationConfig, LLMNodeResult, LLMPart,
    StreamEvent, SwitchCaseContext, SwitchNodeResult, TriggerEvaluation as TriggerEvaluationResult,
    TriggerEvaluationContext, TriggerNodeResult, UserInputNodeResult, WorkflowStateSummary,
};
use models::{
    ErrorHandlerNodeConfig, LLMCallNodeConfig, ResponseFormat, SwitchNodeConfig,
    ToolCallNodeConfig, TriggerNodeConfig, UserInputNodeConfig, UserInputType, WorkflowEdge,
    WorkflowNode, WorkflowNodeType,
};
use queries::{GetToolByIdQuery, Query};
use serde_json::{json, Value};
use std::collections::HashMap;

impl AgentExecutor {
    pub(super) async fn execute_node(
        &self,
        node_type: &WorkflowNodeType,
        all_nodes: &[WorkflowNode],
        all_edges: &[WorkflowEdge],
        workflow_state: &mut HashMap<String, Value>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
        depth: usize,
    ) -> Result<Value, AppError> {
        match node_type {
            WorkflowNodeType::Trigger(config) => {
                self.execute_trigger_node(config, workflow_state).await
            }
            WorkflowNodeType::ErrorHandler(config) => {
                self.execute_error_handler_node(
                    config,
                    all_nodes,
                    all_edges,
                    workflow_state,
                    channel,
                    depth,
                )
                .await
            }
            WorkflowNodeType::LLMCall(config) => self.execute_llm_call_node(config).await,
            WorkflowNodeType::Switch(config) => {
                self.execute_switch_node(config, workflow_state).await
            }
            WorkflowNodeType::ToolCall(config) => {
                self.execute_tool_call_node(config, channel).await
            }
            WorkflowNodeType::UserInput(config) => self.execute_user_input_node(config).await,
        }
    }

    async fn execute_trigger_node(
        &self,
        config: &TriggerNodeConfig,
        workflow_state: &HashMap<String, Value>,
    ) -> Result<Value, AppError> {
        let mut outputs = HashMap::new();
        for (key, value) in workflow_state {
            if key.ends_with("_output") {
                outputs.insert(key.clone(), value.clone());
            }
        }

        let workflow_state_summary = WorkflowStateSummary {
            inputs: workflow_state
                .get("inputs")
                .cloned()
                .unwrap_or(serde_json::json!({})),
            total_context_items: workflow_state
                .get("total_context_items")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32,
            has_conversation_history: workflow_state.contains_key("conversation_history"),
            has_memory_context: workflow_state.contains_key("memory_context"),
            outputs,
        };

        let template_context = TriggerEvaluationContext {
            trigger_condition: config.condition.clone(),
            trigger_description: config.description.clone().unwrap_or_default(),
            workflow_state: workflow_state_summary,
        };

        let request_body = render_template_with_prompt(
            AgentTemplates::TRIGGER_EVALUATION,
            serde_json::to_value(&template_context)?,
        )
        .map_err(|e| {
            AppError::Internal(format!("Failed to render trigger evaluation template: {e}"))
        })?;

        let (evaluation, _) = self
            .create_weak_llm().await?
            .generate_structured_content::<dto::json::agent_responses::TriggerEvaluation>(
                request_body,
            )
            .await?;

        if evaluation.should_trigger {
            let result = TriggerNodeResult {
                node_type: "trigger".to_string(),
                triggered: true,
                description: config.description.clone().unwrap_or_default(),
                trigger_condition: config.condition.clone(),
                evaluation: TriggerEvaluationResult {
                    reasoning: evaluation.reasoning,
                    confidence: evaluation.confidence as f32,
                },
                context: workflow_state.clone(),
            };
            Ok(serde_json::to_value(&result)?)
        } else {
            Err(AppError::BadRequest(format!(
                "Trigger condition not met: {}. Missing requirements: {}",
                evaluation.reasoning,
                evaluation
                    .missing_requirements
                    .unwrap_or_default()
                    .join(", ")
            )))
        }
    }

    async fn execute_error_handler_node(
        &self,
        config: &ErrorHandlerNodeConfig,
        all_nodes: &[WorkflowNode],
        all_edges: &[WorkflowEdge],
        workflow_state: &mut HashMap<String, Value>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
        depth: usize,
    ) -> Result<Value, AppError> {
        let max_retries = if config.enable_retry {
            config.max_retries
        } else {
            0
        };

        for retry_count in 0..=max_retries {
            let execution_result = self
                .execute_contained_nodes(
                    &config.contained_nodes,
                    all_nodes,
                    all_edges,
                    workflow_state,
                    channel.clone(),
                    depth,
                )
                .await;

            match execution_result {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if config.log_errors {}

                    if retry_count < max_retries {
                        tokio::time::sleep(tokio::time::Duration::from_secs(
                            config.retry_delay_seconds as u64,
                        ))
                        .await;
                        continue;
                    }

                    return Err(e);
                }
            }
        }

        Ok(json!({}))
    }

    async fn execute_contained_nodes(
        &self,
        node_ids: &[String],
        all_nodes: &[WorkflowNode],
        all_edges: &[WorkflowEdge],
        workflow_state: &mut HashMap<String, Value>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
        depth: usize,
    ) -> Result<Value, AppError> {
        let mut last_result = json!({});

        for node_id in node_ids {
            let node = all_nodes
                .iter()
                .find(|n| n.id == *node_id)
                .ok_or_else(|| AppError::Internal(format!("Node {node_id} not found")))?;

            last_result = self
                .execute_node_recursive(
                    node,
                    all_nodes,
                    all_edges,
                    workflow_state.clone(),
                    channel.clone(),
                    depth + 1,
                )
                .await?;
        }

        Ok(last_result)
    }

    async fn execute_llm_call_node(&self, config: &LLMCallNodeConfig) -> Result<Value, AppError> {
        let prompt = config.prompt_template.clone();

        let generation_config = LLMGenerationConfig {
            contents: vec![LLMContent {
                parts: vec![LLMPart { text: prompt }],
            }],
            generation_config: GenerationConfig {
                temperature: 0.7,
                top_k: 40,
                top_p: 0.95,
                max_output_tokens: 8192,
            },
        };

        let request_body = serde_json::to_string(&generation_config)?;
        let llm = self.create_weak_llm().await?;
        let (response, _): (Value, _) = llm
            .generate_structured_content(request_body)
            .await
            .map_err(|e| AppError::Internal(format!("LLM call failed: {e}")))?;

        let response_text =
            serde_json::to_string(&response).unwrap_or_else(|_| response.to_string());

        match config.response_format {
            ResponseFormat::Text => {
                let result = LLMNodeResult {
                    node_type: "llm_response".to_string(),
                    format: "text".to_string(),
                    content: response_text,
                    parse_error: None,
                };
                Ok(serde_json::to_value(&result)?)
            }
            ResponseFormat::Json => {
                let result = if serde_json::from_str::<Value>(&response_text).is_ok() {
                    LLMNodeResult {
                        node_type: "llm_response".to_string(),
                        format: "json".to_string(),
                        content: response_text,
                        parse_error: None,
                    }
                } else {
                    LLMNodeResult {
                        node_type: "llm_response".to_string(),
                        format: "json".to_string(),
                        content: response_text,
                        parse_error: Some("Failed to parse response as JSON".to_string()),
                    }
                };
                Ok(serde_json::to_value(&result)?)
            }
        }
    }

    async fn execute_switch_node(
        &self,
        config: &SwitchNodeConfig,
        workflow_state: &HashMap<String, Value>,
    ) -> Result<Value, AppError> {
        let switch_value = serde_json::to_value(&config.switch_condition)?;

        let case_descriptions: Vec<CaseDescription> = config
            .cases
            .iter()
            .enumerate()
            .map(|(index, case)| CaseDescription {
                index,
                label: case.case_label.clone().unwrap_or_default(),
                condition: case.case_condition.clone(),
            })
            .collect();

        let context = SwitchCaseContext {
            switch_value: switch_value.clone(),
            cases: case_descriptions,
            has_default: config.default_case,
            workflow_state: workflow_state.clone(),
        };

        let request_body = render_template_with_prompt(
            AgentTemplates::SWITCH_CASE_EVALUATION,
            serde_json::to_value(&context)?,
        )
        .map_err(|e| AppError::Internal(format!("Failed to render switch case template: {e}")))?;

        let (evaluation, _) = self
            .create_weak_llm().await?
            .generate_structured_content::<SwitchCaseEvaluation>(request_body)
            .await?;

        if let Some(case_index) = evaluation.selected_case_index {
            if case_index < config.cases.len() {
                let result = SwitchNodeResult {
                    node_type: "switch".to_string(),
                    matched_case: serde_json::to_value(case_index)?,
                    case_label: evaluation.selected_case_label,
                    switch_value: switch_value.clone(),
                    reasoning: evaluation.reasoning,
                    confidence: Some(evaluation.confidence as f32),
                };
                return Ok(serde_json::to_value(&result)?);
            }
        }

        if evaluation.use_default && config.default_case {
            let result = SwitchNodeResult {
                node_type: "switch".to_string(),
                matched_case: serde_json::to_value("default")?,
                case_label: None,
                switch_value,
                reasoning: evaluation.reasoning,
                confidence: None,
            };
            Ok(serde_json::to_value(&result)?)
        } else {
            Err(AppError::Internal(format!(
                "No matching case for switch value: {}. Reasoning: {}",
                switch_value, evaluation.reasoning
            )))
        }
    }

    async fn execute_tool_call_node(
        &self,
        config: &ToolCallNodeConfig,
        _channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        let tool = GetToolByIdQuery::new(config.tool_id)
            .execute(&self.ctx.app_state)
            .await?;

        let parameters = config.input_parameters.clone();

        let title = self.ctx.context_title().await?;
        let result = self
            .tool_executor
            .execute_tool_immediately(
                &tool,
                json!(parameters),
                &self.filesystem,
                &self.shell,
                &title,
            )
            .await?;

        Ok(result)
    }

    async fn execute_user_input_node(
        &self,
        config: &UserInputNodeConfig,
    ) -> Result<Value, AppError> {
        {
            let user_input_request = models::ConversationContent::UserInputRequest {
                question: config.prompt.clone(),
                context: config.description.clone().unwrap_or_default(),
                input_type: match &config.input_type {
                    UserInputType::Text => "text",
                    UserInputType::Number => "number",
                    UserInputType::Select => "select",
                    UserInputType::MultiSelect => "multiselect",
                    UserInputType::Boolean => "boolean",
                    UserInputType::Date => "date",
                }
                .to_string(),
                options: config.options.clone(),
                default_value: config.default_value.clone(),
                placeholder: config.placeholder.clone(),
            };

            let event = StreamEvent::UserInputRequest(user_input_request);
            let _ = self.channel.send(event).await;
        }

        let result = UserInputNodeResult {
            status: "pending".to_string(),
            node_type: "user_input".to_string(),
            input_type: match &config.input_type {
                UserInputType::Text => "text",
                UserInputType::Number => "number",
                UserInputType::Select => "select",
                UserInputType::MultiSelect => "multiselect",
                UserInputType::Boolean => "boolean",
                UserInputType::Date => "date",
            }
            .to_string(),
            prompt: config.prompt.clone(),
            options: config.options.clone(),
            default_value: config.default_value.clone(),
            placeholder: config.placeholder.clone(),
        };
        Ok(serde_json::to_value(&result)?)
    }
}
