use crate::agentic::{
    DecayManager, ToolExecutor, context_orchestrator::ContextOrchestrator,
    gemini_client::GeminiClient,
};
use crate::template::{AgentTemplates, render_template_with_prompt};
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;
use serde_json::{Value, json};
use shared::commands::{Command, CreateConversationCommand};
use shared::dto::json::StreamEvent;
use shared::dto::json::agent_executor::{
    AcknowledgmentResponse, ConversationInsights, ConverseRequest, NextStep, ObjectiveDefinition,
    StepDecision, TaskExecutionResult,
};
use shared::dto::json::agent_responses::{
    ActionsList, ExecutableTask, ExecutionAction, ExecutionStatus as TaskExecutionStatus,
    LoopDecision, ParameterGenerationResponse, SwitchCaseEvaluation, TaskBreakdownResponse,
    TaskExecution, TaskExecutionResponse, TaskType, TriggerEvaluation, ValidationResponse,
};
use shared::error::AppError;
use shared::models::{
    AiAgentWithFeatures, AiTool, AiToolConfiguration, AiWorkflow, ApiToolConfiguration,
    ConversationContent, ConversationMessageType, ConversationRecord, MemoryRecord, NodeExecution,
    ResponseFormat, WorkflowEdge, WorkflowNode, WorkflowNodeType,
};
use shared::queries::Query;
use shared::state::AppState;
use std::collections::HashMap;

const MAX_LOOP_ITERATIONS: usize = 50;

pub struct AgentExecutor {
    pub agent: AiAgentWithFeatures,
    pub app_state: AppState,
    pub context_id: i64,
    pub conversations: Vec<ConversationRecord>,
    context_orchestrator: ContextOrchestrator,
    tool_executor: ToolExecutor,
    decay_manager: DecayManager,
    channel: tokio::sync::mpsc::Sender<StreamEvent>,
    memories: Vec<MemoryRecord>,
    user_request: String,
    current_objective: Option<ObjectiveDefinition>,
    conversation_insights: Option<ConversationInsights>,
    executable_tasks: Vec<ExecutableTask>,
    task_results: HashMap<String, TaskExecutionResult>,
}

pub struct AgentExecutorBuilder {
    agent: AiAgentWithFeatures,
    app_state: AppState,
    context_id: i64,
    channel: tokio::sync::mpsc::Sender<StreamEvent>,
}

impl AgentExecutorBuilder {
    pub fn new(
        agent: AiAgentWithFeatures,
        context_id: i64,
        app_state: AppState,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Self {
        Self {
            agent,
            context_id,
            app_state,
            channel,
        }
    }

    pub async fn build(self) -> Result<AgentExecutor, AppError> {
        let tool_executor = ToolExecutor::new();
        let decay_manager = DecayManager::new(self.app_state.clone());

        let context_orchestrator =
            ContextOrchestrator::new(self.app_state.clone(), self.agent.clone());

        Ok(AgentExecutor {
            agent: self.agent,
            app_state: self.app_state,
            context_id: self.context_id,
            context_orchestrator,
            tool_executor,
            user_request: String::new(),
            decay_manager,
            channel: self.channel,
            memories: Vec::new(),
            conversations: Vec::new(),
            current_objective: None,
            conversation_insights: None,
            executable_tasks: Vec::new(),
            task_results: HashMap::new(),
        })
    }
}

impl AgentExecutor {
    pub async fn new(
        agent: AiAgentWithFeatures,
        context_id: i64,
        app_state: AppState,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Self, AppError> {
        AgentExecutorBuilder::new(agent, context_id, app_state, channel)
            .build()
            .await
    }

    pub async fn execute_with_streaming(&mut self, message: String) -> Result<(), AppError> {
        let request = ConverseRequest { message };
        self.run(request).await
    }

    pub async fn run(&mut self, request: ConverseRequest) -> Result<(), AppError> {
        self.user_request = request.message.clone();
        self.store_conversation(
            ConversationContent::UserMessage {
                message: request.message,
            },
            ConversationMessageType::UserMessage,
        )
        .await?;

        let context = self
            .decay_manager
            .get_immediate_context(self.context_id)
            .await?;
        self.conversations = context.conversations;
        self.memories = context.memories;

        let ack = self.generate_acknowledgment().await?;
        if !ack.further_action_required {
            return Ok(());
        }

        self.repl().await?;

        Ok(())
    }

    async fn repl(&mut self) -> Result<(), AppError> {
        let mut iteration = 0;
        loop {
            iteration += 1;
            if iteration > MAX_LOOP_ITERATIONS {
                self.generate_and_send_summary().await?;
                return Ok(());
            }

            let decision = self.decide_next_step().await?;

            match decision.next_step {
                NextStep::Acknowledge => {}

                NextStep::GatherContext => match self.gather_context().await {
                    Ok(_) => println!("Context gathering completed successfully"),
                    Err(e) => {
                        return Err(e);
                    }
                },

                NextStep::DirectExecution => {
                    if let Some(action) = decision.direct_execution {
                        let result = self.execute_action(&action).await?;

                        let execution = TaskExecution {
                            approach: format!("Direct execution: {}", action.purpose),
                            actions: ActionsList {
                                actions: vec![action.clone()],
                            },
                            expected_result: "Direct execution result".to_string(),
                            actual_result: Some(result.clone()),
                        };

                        self.store_conversation(
                            ConversationContent::AssistantTaskExecution {
                                task_execution: serde_json::to_value(&execution)?,
                                execution_status: "completed".to_string(),
                                blocking_reason: None,
                            },
                            ConversationMessageType::AssistantTaskExecution,
                        )
                        .await?;
                    }
                }

                NextStep::BreakdownTasks => {
                    self.breakdown_tasks().await?;
                }

                NextStep::ExecuteTasks => {
                    if let Err(e) = self.execute_pending_tasks().await {
                        self.store_conversation(
                            ConversationContent::SystemDecision {
                                step: "task_execution_error".to_string(),
                                reasoning: format!("Task execution failed: {}", e),
                                confidence: 1.0,
                            },
                            ConversationMessageType::SystemDecision,
                        )
                        .await?;
                    }
                }

                NextStep::ValidateProgress => {
                    let validation_result = self.validate_execution().await?;
                    if validation_result.validation_result.overall_success {
                        self.generate_and_send_summary().await?;
                        return Ok(());
                    }
                }

                NextStep::DeliverResponse => {
                    self.generate_and_send_summary().await?;
                    return Ok(());
                }

                NextStep::RequestUserInput => {
                    eprintln!("User input request not yet implemented");
                }

                NextStep::Complete => {
                    return Ok(());
                }
            }
        }
    }

    async fn decide_next_step(&mut self) -> Result<StepDecision, AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::STEP_DECISION,
            json!({
                "conversation_history": self.get_conversation_history_for_llm(),
                "user_request": self.user_request,
                "current_objective": self.current_objective,
                "conversation_insights": self.conversation_insights,
                "executable_tasks": self.executable_tasks,
                "task_results": self.task_results,
                "available_tools": self.agent.tools.clone(),
                "available_workflows": self.agent.workflows.clone(),
                "available_knowledge_bases": self.agent.knowledge_bases.clone(),
                "iteration_info": {
                    "current_iteration": 1,
                    "max_iterations": MAX_LOOP_ITERATIONS,
                },
            }),
        )
        .map_err(|e| {
            AppError::Internal(format!("Failed to render step decision template: {}", e))
        })?;

        let decision = self
            .create_main_llm()?
            .generate_structured_content::<StepDecision>(request_body)
            .await?;

        self.store_conversation(
            ConversationContent::SystemDecision {
                step: "decide_next_step".to_string(),
                reasoning: decision.reasoning.clone(),
                confidence: 0.8,
            },
            ConversationMessageType::SystemDecision,
        )
        .await?;

        Ok(decision)
    }

    async fn generate_acknowledgment(&mut self) -> Result<AcknowledgmentResponse, AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::ACKNOWLEDGMENT,
            json!({
                "conversation_history": self.get_conversation_history_for_llm(),
                "agent_name": self.agent.name,
                "agent_description": self.agent.description.as_ref().unwrap_or(&"".to_string()),
                "tools": self.agent.tools.clone(),
                "workflows": self.agent.workflows.clone(),
                "knowledge_bases": self.agent.knowledge_bases.clone(),
                "memories": self.memories.clone(),
            }),
        )
        .map_err(|e| {
            AppError::Internal(format!("Failed to render acknowledgment template: {}", e))
        })?;

        let ack = self
            .create_main_llm()?
            .generate_structured_content::<AcknowledgmentResponse>(request_body)
            .await?;

        self.store_conversation(
            ConversationContent::AssistantAcknowledgment {
                acknowledgment_message: ack.message.clone(),
                further_action_required: ack.further_action_required,
                reasoning: ack.reasoning.clone(),
            },
            ConversationMessageType::AssistantAcknowledgment,
        )
        .await?;

        self.current_objective = Some(ack.objective.clone());
        self.conversation_insights = Some(ack.conversation_insights.clone());

        Ok(ack)
    }

    async fn breakdown_tasks(&mut self) -> Result<(), AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::TASK_BREAKDOWN,
            json!({
                "conversation_history": self.get_conversation_history_for_llm(),
                "user_request": self.user_request,
                "current_objective": self.current_objective,
                "conversation_insights": self.conversation_insights,
                "available_tools": self.agent.tools.clone(),
                "workflows": self.agent.workflows.clone(),
            }),
        )
        .map_err(|e| {
            AppError::Internal(format!("Failed to render task breakdown template: {}", e))
        })?;

        let breakdown = self
            .create_main_llm()?
            .generate_structured_content::<TaskBreakdownResponse>(request_body)
            .await?;

        let breakdown_value = serde_json::to_value(&breakdown)?;

        self.store_conversation(
            ConversationContent::AssistantActionPlanning {
                task_execution: breakdown_value.clone(),
                execution_status: "ready".to_string(),
                blocking_reason: None,
            },
            ConversationMessageType::AssistantActionPlanning,
        )
        .await?;

        self.executable_tasks = breakdown.tasks;

        Ok(())
    }

    async fn validate_execution(&mut self) -> Result<ValidationResponse, AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::VALIDATION,
            json!({
                "conversation_history": self.get_conversation_history_for_llm(),
                "user_request": self.user_request,
                "current_objective": self.current_objective,
                "task_results": self.task_results,
                "executable_tasks": self.executable_tasks,
            }),
        )
        .map_err(|e| AppError::Internal(format!("Failed to render validation template: {}", e)))?;

        let validation = self
            .create_main_llm()?
            .generate_structured_content::<ValidationResponse>(request_body)
            .await?;

        self.store_conversation(
            ConversationContent::AssistantValidation {
                validation_result: serde_json::to_value(&validation.validation_result)?,
                loop_decision: match validation.loop_decision {
                    LoopDecision::Continue => "continue".to_string(),
                    LoopDecision::Complete => "complete".to_string(),
                    LoopDecision::AbortUnresolvable => "abort_unresolvable".to_string(),
                },
                decision_reasoning: validation.decision_reasoning.clone(),
                next_iteration_focus: validation.next_iteration_focus.clone(),
                has_unresolvable_errors: validation.has_unresolvable_errors,
                unresolvable_error_details: validation.unresolvable_error_details.clone(),
            },
            ConversationMessageType::AssistantValidation,
        )
        .await?;

        Ok(validation)
    }

    async fn generate_and_send_summary(&mut self) -> Result<(), AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::SUMMARY,
            json!({
                "conversation_history": self.get_conversation_history_for_llm(),
                "user_request": self.user_request,
                "task_results": self.task_results,
            }),
        )
        .map_err(|e| AppError::Internal(format!("Failed to render summary template: {}", e)))?;

        let summary = self
            .create_main_llm()?
            .generate_structured_content::<Value>(request_body)
            .await?;

        self.store_conversation(
            ConversationContent::AgentResponse {
                response: summary.get("response").unwrap().as_str().unwrap().into(),
                citations: Default::default(),
                context_used: Default::default(),
            },
            ConversationMessageType::AgentResponse,
        )
        .await?;

        Ok(())
    }

    async fn gather_context(&mut self) -> Result<(), AppError> {
        let context_results = match self
            .context_orchestrator
            .gather_context(&self.conversations, &self.current_objective)
            .await
        {
            Ok(results) => results,
            Err(e) => {
                return Err(e);
            }
        };

        println!("Storing context results in conversation...");

        self.store_conversation(
            ConversationContent::ContextResults {
                query: "Context gathering completed".to_string(),
                results: serde_json::to_value(&context_results)?,
                result_count: context_results.len(),
                timestamp: chrono::Utc::now(),
            },
            ConversationMessageType::ContextResults,
        )
        .await?;

        println!("=== Agent executor gather_context completed ===");
        Ok(())
    }

    async fn execute_pending_tasks(&mut self) -> Result<(), AppError> {
        if self.executable_tasks.is_empty() {
            return Err(AppError::Internal(
                "No tasks to execute, did we breakdown the tasks first?".to_string(),
            ));
        }

        let task_to_execute = self
            .executable_tasks
            .iter()
            .find(|t| {
                !self.task_results.contains_key(&t.id)
                    && t.dependencies
                        .iter()
                        .all(|dep| self.task_results.contains_key(dep))
            })
            .cloned();

        if let Some(task) = task_to_execute {
            let result = self.execute_single_task(&task).await;

            let task_result = match result {
                Ok(value) => TaskExecutionResult {
                    task_id: task.id.clone(),
                    status: "completed".to_string(),
                    output: Some(value),
                    error: None,
                },
                Err(e) => TaskExecutionResult {
                    task_id: task.id.clone(),
                    status: "failed".to_string(),
                    output: None,
                    error: Some(e.to_string()),
                },
            };

            self.task_results.insert(task.id.clone(), task_result);
        }

        Ok(())
    }

    async fn execute_single_task(&mut self, task: &ExecutableTask) -> Result<Value, AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::TASK_EXECUTION,
            json!({
                "task": task,
                "conversation_history": self.get_conversation_history_for_llm(),
                "task_results": self.task_results,
            }),
        )
        .map_err(|e| {
            AppError::Internal(format!("Failed to render task execution template: {}", e))
        })?;

        let execution = self
            .create_main_llm()?
            .generate_structured_content::<TaskExecutionResponse>(request_body)
            .await?;

        let result = if execution.execution_status == TaskExecutionStatus::Ready {
            if let Some(action) = execution.task_execution.actions.actions.first() {
                match self.execute_action(action).await {
                    Ok(value) => value,
                    Err(e) => {
                        let error_result = json!({
                            "error": e.to_string(),
                            "error_type": "execution_failure"
                        });

                        let mut task_execution_with_error = execution.task_execution.clone();
                        task_execution_with_error.actual_result = Some(error_result.clone());

                        self.store_conversation(
                            ConversationContent::AssistantTaskExecution {
                                task_execution: serde_json::to_value(&task_execution_with_error)?,
                                execution_status: "failed".to_string(),
                                blocking_reason: Some(e.to_string()),
                            },
                            ConversationMessageType::AssistantTaskExecution,
                        )
                        .await?;

                        return Err(e);
                    }
                }
            } else {
                json!({"error": "No action to execute"})
            }
        } else {
            json!({
                "error": "Execution blocked or cannot execute",
                "reason": execution.blocking_reason
            })
        };

        let mut task_execution_with_result = execution.task_execution.clone();
        task_execution_with_result.actual_result = Some(result.clone());

        self.store_conversation(
            ConversationContent::AssistantTaskExecution {
                task_execution: serde_json::to_value(&task_execution_with_result)?,
                execution_status: "completed".to_string(),
                blocking_reason: None,
            },
            ConversationMessageType::AssistantTaskExecution,
        )
        .await?;

        Ok(result)
    }

    async fn execute_action(&self, action: &ExecutionAction) -> Result<Value, AppError> {
        match &action.action_type {
            TaskType::ToolCall => {
                let tool_call = self.parse_tool_call(&action.details).await?;
                let tool = self
                    .agent
                    .tools
                    .iter()
                    .find(|t| t.name == tool_call.tool_name)
                    .ok_or_else(|| {
                        AppError::BadRequest(format!("Tool '{}' not found", tool_call.tool_name))
                    })?;
                self.tool_executor
                    .execute_tool_immediately(tool, tool_call.parameters)
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
                            "learning_confidence": mem.learning_confidence,
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
        }
    }

    fn schema_fields_to_properties(fields: &[shared::models::SchemaField]) -> (Value, Vec<String>) {
        let mut properties = serde_json::Map::new();
        let mut required_fields = Vec::new();

        for field in fields {
            let mut field_def = serde_json::Map::new();
            field_def.insert("type".to_string(), json!(field.field_type.to_lowercase()));

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

        let field_in_schema =
            |field_name: &str, schema: &Option<Vec<shared::models::SchemaField>>| {
                schema
                    .as_ref()
                    .map_or(false, |fields| fields.iter().any(|f| f.name == field_name))
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

    async fn parse_tool_call(
        &self,
        details: &Value,
    ) -> Result<shared::dto::json::ToolCall, AppError> {
        let tool_name = details["tool_name"]
            .as_str()
            .ok_or_else(|| AppError::BadRequest("Tool name not specified".to_string()))?;

        let tool = self
            .agent
            .tools
            .iter()
            .find(|t| t.name == tool_name)
            .ok_or_else(|| AppError::BadRequest(format!("Tool '{}' not found", tool_name)))?;

        let needs_llm_params = match &tool.configuration {
            AiToolConfiguration::Api(api_config) => {
                api_config.request_body_schema.is_some() || api_config.url_params_schema.is_some()
            }
            AiToolConfiguration::PlatformFunction(func_config) => {
                func_config.input_schema.is_some()
            }
            _ => false,
        };

        let params = if needs_llm_params {
            let generated_params = self.generate_tool_parameters(tool).await?;

            // For API tools, organize parameters into url_params, query_params, and body
            if let AiToolConfiguration::Api(api_config) = &tool.configuration {
                self.organize_api_parameters(generated_params, api_config)?
            } else {
                generated_params
            }
        } else {
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
        };

        Ok(shared::dto::json::ToolCall {
            tool_name: tool_name.to_string(),
            parameters: params,
        })
    }

    async fn generate_tool_parameters(&self, tool: &AiTool) -> Result<Value, AppError> {
        // ParameterGenerationResponse is already imported at the top

        let parameter_schema = match &tool.configuration {
            AiToolConfiguration::Api(api_config) => {
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
                    println!(
                        "Tool {} has no parameters defined, returning empty params",
                        tool.name
                    );
                    return Ok(json!({}));
                }

                json!({
                    "type": "OBJECT",
                    "properties": all_properties,
                    "required": all_required
                })
            }
            AiToolConfiguration::PlatformFunction(func_config) => {
                if let Some(schema) = &func_config.input_schema {
                    let (properties, required) = Self::schema_fields_to_properties(schema);

                    let is_empty = properties.as_object().map_or(true, |p| p.is_empty());
                    if is_empty {
                        return Ok(json!({}));
                    }

                    json!({
                        "type": "OBJECT",
                        "properties": properties,
                        "required": required
                    })
                } else {
                    return Ok(json!({}));
                }
            }
            _ => {
                return Err(AppError::Internal(
                    "Parameter generation called for non-API/PlatformFunction tool".to_string(),
                ));
            }
        };

        let request_body = render_template_with_prompt(
            AgentTemplates::PARAMETER_GENERATION,
            json!({
                "conversation_history": self.get_conversation_history_for_llm(),
                "tool_name": tool.name,
                "tool_description": tool.description.as_ref().unwrap_or(&"".to_string()),
                "parameter_schema": parameter_schema,
                "user_request": self.user_request,
                "current_objective": self.current_objective,
                "conversation_insights": self.conversation_insights,
            }),
        )
        .map_err(|e| {
            AppError::Internal(format!(
                "Failed to render parameter generation template: {}",
                e
            ))
        })?;

        let response = self
            .create_main_llm()?
            .generate_structured_content::<ParameterGenerationResponse>(request_body)
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

    fn parse_workflow_call(
        &self,
        details: &Value,
    ) -> Result<shared::dto::json::WorkflowCall, AppError> {
        let workflow_name = details["workflow_name"]
            .as_str()
            .ok_or_else(|| AppError::BadRequest("Workflow name not specified".to_string()))?;

        let inputs = details.get("inputs").cloned().unwrap_or(json!({}));

        Ok(shared::dto::json::WorkflowCall {
            workflow_name: workflow_name.to_string(),
            inputs,
        })
    }

    async fn store_conversation(
        &mut self,
        content: ConversationContent,
        message_type: ConversationMessageType,
    ) -> Result<(), AppError> {
        let command = CreateConversationCommand::new(
            self.app_state.sf.next_id()? as i64,
            self.context_id,
            content,
            message_type,
        );
        let conversation = command.execute(&self.app_state).await?;
        self.conversations.push(conversation.clone());

        let _ = self
            .channel
            .send(StreamEvent::ConversationMessage(conversation))
            .await;

        Ok(())
    }

    fn get_conversation_history_for_llm(&self) -> Vec<Value> {
        self.conversations
            .iter()
            .map(|conv| {
                json!({
                    "role": self.map_conversation_type_to_role(&conv.message_type),
                    "content": self.extract_conversation_content(&conv.content),
                    "timestamp": conv.created_at,
                    "type": conv.message_type,
                })
            })
            .collect()
    }

    fn map_conversation_type_to_role(&self, msg_type: &ConversationMessageType) -> &'static str {
        match msg_type {
            ConversationMessageType::UserMessage => "user",
            _ => "model",
        }
    }

    fn extract_conversation_content(&self, content: &ConversationContent) -> String {
        serde_json::to_string(content).unwrap()
    }

    fn create_main_llm(&self) -> Result<GeminiClient, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_else(|_| "test-key".to_string());
        Ok(GeminiClient::new(
            api_key,
            Some("gemini-2.5-flash".to_string()),
        ))
    }

    pub async fn execute_workflow_task(
        &self,
        workflow_call: &shared::dto::json::WorkflowCall,
        workflows: &[AiWorkflow],
        conversation_context: &[Value],
        memory_context: &[Value],
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        let workflow = workflows
            .iter()
            .find(|w| w.name == workflow_call.workflow_name)
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Workflow {} not found",
                    workflow_call.workflow_name
                ))
            })?;

        let mut initial_state = workflow_call.inputs.clone();
        if let Some(obj) = initial_state.as_object_mut() {
            obj.insert(
                "conversation_history".to_string(),
                json!(conversation_context),
            );
            obj.insert("memory_context".to_string(), json!(memory_context));
            obj.insert(
                "total_context_items".to_string(),
                json!(conversation_context.len() + memory_context.len()),
            );
        }

        let result = self
            .execute_workflow(workflow, initial_state, channel)
            .await;

        match &result {
            Ok(res) => {
                tracing::info!("Workflow {} completed successfully", workflow.name);
                tracing::debug!("Workflow result: {:?}", res);
            }
            Err(e) => {
                tracing::error!("Workflow {} failed: {}", workflow.name, e);
            }
        }

        result
    }

    pub async fn execute_workflow(
        &self,
        workflow: &AiWorkflow,
        input_data: Value,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        let trigger_node = workflow
            .workflow_definition
            .nodes
            .iter()
            .find(|node| matches!(node.node_type, WorkflowNodeType::Trigger(_)))
            .ok_or_else(|| AppError::BadRequest("No trigger node found in workflow".to_string()))?;

        let mut workflow_state = HashMap::new();
        if let Some(input_obj) = input_data.as_object() {
            for (key, value) in input_obj {
                workflow_state.insert(key.clone(), value.clone());
            }
        }

        let output = self
            .execute_node_recursive(
                trigger_node,
                &workflow.workflow_definition.nodes,
                &workflow.workflow_definition.edges,
                workflow_state,
                channel.clone(),
                0,
            )
            .await?;

        Ok(json!({
            "workflow_id": workflow.id,
            "workflow_name": workflow.name,
            "execution_status": "completed",
            "output": output,
        }))
    }

    fn execute_node_recursive<'a>(
        &'a self,
        node: &'a WorkflowNode,
        all_nodes: &'a [WorkflowNode],
        all_edges: &'a [WorkflowEdge],
        mut workflow_state: HashMap<String, Value>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
        depth: usize,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, AppError>> + Send + 'a>>
    {
        Box::pin(async move {
            if depth > 100 {
                return Err(AppError::Internal(
                    "Workflow execution depth limit exceeded".to_string(),
                ));
            }

            let result = match &node.node_type {
                WorkflowNodeType::Trigger(config) => {
                    self.execute_trigger_node(config, &workflow_state).await
                }
                WorkflowNodeType::ErrorHandler(config) => {
                    self.execute_error_handler_node(
                        config,
                        all_nodes,
                        all_edges,
                        &mut workflow_state,
                        channel.clone(),
                        depth,
                    )
                    .await
                }
                WorkflowNodeType::LLMCall(config) => self.execute_llm_call_node(config).await,
                WorkflowNodeType::Switch(config) => {
                    self.execute_switch_node(config, &workflow_state).await
                }
                WorkflowNodeType::ToolCall(config) => {
                    self.execute_tool_call_node(config, channel.clone()).await
                }
            };

            match result {
                Ok(output) => {
                    // Store node output in workflow state
                    workflow_state.insert(format!("{}_output", node.id), output.clone());

                    let next_edges: Vec<&WorkflowEdge> = all_edges
                        .iter()
                        .filter(|edge| edge.source == node.id)
                        .collect();

                    // Handle switch nodes specially
                    if node.node_type.type_name() == "Switch" {
                        // For switch nodes, find the edge matching the selected case
                        if let Some(matched_case) = output.get("matched_case") {
                            let case_handle = if matched_case == "default" {
                                "default".to_string()
                            } else if let Some(case_idx) = matched_case.as_u64() {
                                format!("case-{}", case_idx)
                            } else {
                                return Ok(output);
                            };

                            if let Some(matching_edge) = next_edges
                                .into_iter()
                                .find(|e| e.source_handle.as_ref() == Some(&case_handle))
                            {
                                if let Some(next_node) =
                                    all_nodes.iter().find(|n| n.id == matching_edge.target)
                                {
                                    return self
                                        .execute_node_recursive(
                                            next_node,
                                            all_nodes,
                                            all_edges,
                                            workflow_state.clone(),
                                            channel.clone(),
                                            depth + 1,
                                        )
                                        .await;
                                }
                            }
                        }
                        Ok(output)
                    } else if next_edges.len() == 1 {
                        // Regular nodes - follow the single edge
                        let edge = &next_edges[0];
                        if let Some(next_node) = all_nodes.iter().find(|n| n.id == edge.target) {
                            self.execute_node_recursive(
                                next_node,
                                all_nodes,
                                all_edges,
                                workflow_state.clone(),
                                channel.clone(),
                                depth + 1,
                            )
                            .await
                        } else {
                            Ok(output)
                        }
                    } else if next_edges.is_empty() {
                        // No more edges - workflow complete
                        Ok(output)
                    } else {
                        // Multiple edges on non-switch node is an error
                        Err(AppError::BadRequest(format!(
                            "Node {} has multiple outgoing edges but is not a switch node",
                            node.id
                        )))
                    }
                }
                Err(e) => Err(e),
            }
        })
    }

    async fn execute_trigger_node(
        &self,
        config: &shared::models::TriggerNodeConfig,
        workflow_state: &HashMap<String, Value>,
    ) -> Result<Value, AppError> {
        // Extract only the necessary context for trigger evaluation
        let mut context_summary = json!({
            "inputs": workflow_state.get("inputs").cloned().unwrap_or(json!({})),
            "total_context_items": workflow_state.get("total_context_items").cloned().unwrap_or(json!(0)),
            "has_conversation_history": workflow_state.contains_key("conversation_history"),
            "has_memory_context": workflow_state.contains_key("memory_context"),
        });

        // Include node outputs if any
        for (key, value) in workflow_state {
            if key.ends_with("_output") {
                context_summary[key] = value.clone();
            }
        }

        // Use LLM to evaluate if trigger condition is met
        let template_context = json!({
            "trigger_condition": config.condition,
            "trigger_description": config.description,
            "workflow_state": context_summary,
        });

        let request_body =
            render_template_with_prompt(AgentTemplates::TRIGGER_EVALUATION, template_context)
                .map_err(|e| {
                    AppError::Internal(format!(
                        "Failed to render trigger evaluation template: {}",
                        e
                    ))
                })?;

        let evaluation = self
            .create_main_llm()?
            .generate_structured_content::<TriggerEvaluation>(request_body)
            .await?;

        if evaluation.should_trigger {
            Ok(json!({
                "type": "trigger",
                "triggered": true,
                "description": config.description,
                "trigger_condition": config.condition,
                "evaluation": {
                    "reasoning": evaluation.reasoning,
                    "confidence": evaluation.confidence,
                },
                "context": workflow_state,
            }))
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
        config: &shared::models::ErrorHandlerNodeConfig,
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
                    if config.log_errors {
                        eprintln!("Error handler: Attempt {} failed: {}", retry_count + 1, e);
                    }

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
                .ok_or_else(|| AppError::Internal(format!("Node {} not found", node_id)))?;

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

    async fn execute_llm_call_node(
        &self,
        config: &shared::models::LLMCallNodeConfig,
    ) -> Result<Value, AppError> {
        let prompt = config.prompt_template.clone();

        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.5-pro")
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to create LLM: {}", e)))?;

        let messages = vec![ChatMessage::user().content(&prompt).build()];

        let response = llm
            .chat(&messages)
            .await
            .map_err(|e| AppError::Internal(format!("LLM call failed: {}", e)))?;

        let response_text = response.to_string();

        match config.response_format {
            ResponseFormat::Text => Ok(json!({
                "type": "llm_response",
                "format": "text",
                "content": response_text,
            })),
            ResponseFormat::Json => serde_json::from_str(&response_text).or_else(|_| {
                Ok(json!({
                    "type": "llm_response",
                    "format": "json",
                    "content": response_text,
                    "parse_error": "Failed to parse response as JSON"
                }))
            }),
        }
    }

    async fn execute_switch_node(
        &self,
        config: &shared::models::SwitchNodeConfig,
        workflow_state: &HashMap<String, Value>,
    ) -> Result<Value, AppError> {
        // Switch condition is a static description for the LLM to evaluate
        let switch_value = json!(config.switch_condition);

        // Prepare the case options for the LLM
        let case_descriptions: Vec<Value> = config
            .cases
            .iter()
            .enumerate()
            .map(|(index, case)| {
                json!({
                    "index": index,
                    "label": case.case_label,
                    "condition": case.case_condition,
                })
            })
            .collect();

        let request_body = render_template_with_prompt(
            AgentTemplates::SWITCH_CASE_EVALUATION,
            json!({
                "switch_value": switch_value,
                "cases": case_descriptions,
                "has_default": config.default_case,
                "workflow_state": workflow_state,
            }),
        )
        .map_err(|e| AppError::Internal(format!("Failed to render switch case template: {}", e)))?;

        let evaluation = self
            .create_main_llm()?
            .generate_structured_content::<SwitchCaseEvaluation>(request_body)
            .await?;

        if let Some(case_index) = evaluation.selected_case_index {
            if case_index < config.cases.len() {
                return Ok(json!({
                    "type": "switch",
                    "matched_case": case_index,
                    "case_label": evaluation.selected_case_label,
                    "switch_value": switch_value,
                    "reasoning": evaluation.reasoning,
                    "confidence": evaluation.confidence,
                }));
            }
        }

        if evaluation.use_default && config.default_case {
            Ok(json!({
                "type": "switch",
                "matched_case": "default",
                "switch_value": switch_value,
                "reasoning": evaluation.reasoning,
            }))
        } else {
            Err(AppError::Internal(format!(
                "No matching case for switch value: {}. Reasoning: {}",
                switch_value, evaluation.reasoning
            )))
        }
    }

    async fn execute_tool_call_node(
        &self,
        config: &shared::models::ToolCallNodeConfig,
        _channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        let tool = shared::queries::GetToolByIdQuery::new(config.tool_id)
            .execute(&self.app_state)
            .await?;

        let parameters = config.input_parameters.clone();

        let result = self
            .tool_executor
            .execute_tool_immediately(&tool, json!(parameters))
            .await?;

        Ok(result)
    }
}
