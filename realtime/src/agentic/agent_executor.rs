use crate::agentic::{
    ContextGatheringOrchestrator, DecayManager, ExecutableTask, ExecutionAction,
    SharedExecutionContext, TaskBreakdownResponse, TaskExecutionResponse, TaskType, ToolExecutor,
    ValidationResponse, WorkflowExecutor, gemini_client::GeminiClient,
};
use crate::template::{AgentTemplates, render_template_with_prompt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared::commands::{Command, CreateConversationCommand};
use shared::dto::json::StreamEvent;
use shared::error::AppError;
use shared::models::{
    AiAgentWithFeatures, AiTool, AiToolConfiguration, ConversationContent, ConversationMessageType,
    ConversationRecord, MemoryRecordV2,
};
use shared::state::AppState;
use std::collections::HashMap;

const MAX_LOOP_ITERATIONS: usize = 50;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct StepDecision {
    pub next_step: NextStep,
    pub reasoning: String,
    pub ready_to_proceed: bool,
    pub blocking_reason: Option<String>,
    pub progress_assessment: ProgressAssessment,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum NextStep {
    Acknowledge,
    GatherContext,
    BreakdownTasks,
    ExecuteTasks,
    Validate,
    GenerateSummary,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ProgressAssessment {
    pub percentage_complete: u8,
    pub tasks_completed: u32,
    pub tasks_total: u32,
    pub current_phase: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectiveDefinition {
    pub objective: String,
    pub success_criteria: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationInsights {
    pub key_points: Vec<String>,
    pub inferred_objective: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskExecutionResult {
    pub task_id: String,
    pub success: bool,
    pub result: Value,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ConverseRequest {
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AcknowledgmentResponse {
    pub confirmation: String,
    pub understood_request: String,
    pub initial_thoughts: String,
    pub expected_approach: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdeationResponse {
    pub reasoning_summary: String,
    pub needs_more_iteration: bool,
    pub context_search_request: Option<String>,
    pub requires_user_input: bool,
    pub user_input_request: Option<String>,
    pub execution_plan: ExecutionPlan,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub analysis: PlanAnalysis,
    pub success_criteria: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanAnalysis {
    pub understanding: String,
    pub approach: String,
    pub tradeoffs: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowValidationResult {
    pub is_valid: bool,
    pub missing_requirements: Vec<String>,
    pub validation_message: String,
}

pub struct AgentExecutor {
    pub agent: AiAgentWithFeatures,
    pub app_state: AppState,
    pub context_id: i64,
    pub conversations: Vec<ConversationRecord>,
    context_orchestrator: ContextGatheringOrchestrator,
    tool_executor: ToolExecutor,
    workflow_executor: WorkflowExecutor,
    decay_manager: DecayManager,
    channel: tokio::sync::mpsc::Sender<StreamEvent>,
    memories: Vec<MemoryRecordV2>,
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
        let shared_context = SharedExecutionContext::new(
            self.app_state.clone(),
            self.context_id,
            self.agent.clone(),
        );

        let tool_executor = ToolExecutor::new(shared_context.clone());
        let workflow_executor = WorkflowExecutor::new(shared_context.clone());
        let decay_manager = DecayManager::new(self.app_state.clone());

        let context_orchestrator = ContextGatheringOrchestrator::new(
            self.app_state.clone(),
            self.context_id,
            self.agent.clone(),
        );

        Ok(AgentExecutor {
            agent: self.agent,
            app_state: self.app_state,
            context_id: self.context_id,
            context_orchestrator,
            tool_executor,
            workflow_executor,
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

        let context = self
            .decay_manager
            .get_immediate_context(self.context_id)
            .await?;
        self.conversations = context.conversations;
        self.memories = context.memories;

        self.generate_acknowledgment().await?;
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
                NextStep::GatherContext => {
                    self.gather_context().await?;
                }

                NextStep::BreakdownTasks => {
                    self.execute_task_breakdown_step().await?;
                }

                NextStep::ExecuteTasks => {
                    self.execute_pending_tasks().await?;
                }

                NextStep::Validate => {
                    let validation_result = self.validate_execution().await?;
                    if validation_result.validation_result.overall_success {
                        self.generate_and_send_summary().await?;
                        return Ok(());
                    }
                }

                NextStep::GenerateSummary => {
                    self.generate_and_send_summary().await?;
                    return Ok(());
                }

                _ => {}
            }
        }
    }

    async fn decide_next_step(&mut self) -> Result<StepDecision, AppError> {
        let context = json!({
            "conversation_history": self.get_conversation_history_for_llm(),
            "user_request": self.user_request,
            "current_objective": self.current_objective,
            "conversation_insights": self.conversation_insights,
            "executable_tasks": self.executable_tasks,
            "task_results": self.task_results,
        });

        let request_body = render_template_with_prompt(AgentTemplates::STEP_DECISION, &context)
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

    async fn generate_acknowledgment(&mut self) -> Result<(), AppError> {
        let context = json!({
            "user_request": self.user_request,
            "agent_name": self.agent.name,
            "agent_description": "",
            "available_tools": self.agent.tools.iter().map(|t| &t.name).collect::<Vec<_>>(),
            "available_workflows": self.agent.workflows.iter().map(|w| &w.name).collect::<Vec<_>>(),
        });

        let request_body = render_template_with_prompt(AgentTemplates::ACKNOWLEDGMENT, &context)
            .map_err(|e| {
                AppError::Internal(format!("Failed to render acknowledgment template: {}", e))
            })?;

        let ack = self
            .create_main_llm()?
            .generate_structured_content::<AcknowledgmentResponse>(request_body)
            .await?;

        self.store_conversation(
            ConversationContent::AssistantAcknowledgment {
                acknowledgment_message: ack.confirmation.clone(),
                further_action_required: true,
                reasoning: ack.understood_request.clone(),
            },
            ConversationMessageType::AssistantAcknowledgment,
        )
        .await?;

        self.emit_event(&ack).await?;

        Ok(())
    }

    async fn execute_task_breakdown_step(&mut self) -> Result<(), AppError> {
        let context = json!({
            "conversation_history": self.get_conversation_history_for_llm(),
            "user_request": self.user_request,
            "current_objective": self.current_objective,
            "conversation_insights": self.conversation_insights,
        });

        let request_body = render_template_with_prompt(AgentTemplates::TASK_BREAKDOWN, &context)
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

        self.emit_event(&breakdown).await?;

        self.executable_tasks = breakdown.tasks;

        Ok(())
    }

    async fn validate_execution(&mut self) -> Result<ValidationResponse, AppError> {
        let context = json!({
            "conversation_history": self.get_conversation_history_for_llm(),
            "user_request": self.user_request,
            "current_objective": self.current_objective,
            "task_results": self.task_results,
            "executable_tasks": self.executable_tasks,
        });

        let request_body = render_template_with_prompt(AgentTemplates::VALIDATION, &context)
            .map_err(|e| {
                AppError::Internal(format!("Failed to render validation template: {}", e))
            })?;

        let validation = self
            .create_main_llm()?
            .generate_structured_content::<ValidationResponse>(request_body)
            .await?;

        self.store_conversation(
            ConversationContent::AssistantValidation {
                validation_result: serde_json::to_value(&validation.validation_result)?,
                loop_decision: match validation.loop_decision {
                    crate::agentic::LoopDecision::Continue => "continue".to_string(),
                    crate::agentic::LoopDecision::Complete => "complete".to_string(),
                    crate::agentic::LoopDecision::AbortUnresolvable => {
                        "abort_unresolvable".to_string()
                    }
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

    async fn generate_and_send_summary(&self) -> Result<(), AppError> {
        let context = json!({
            "conversation_history": self.get_conversation_history_for_llm(),
            "user_request": self.user_request,
            "task_results": self.task_results,
        });

        let request_body = render_template_with_prompt(AgentTemplates::SUMMARY, &context)
            .map_err(|e| AppError::Internal(format!("Failed to render summary template: {}", e)))?;

        let summary = self
            .create_main_llm()?
            .generate_structured_content::<Value>(request_body)
            .await?;

        self.emit_event(&json!({
            "type": "summary",
            "content": summary,
        }))
        .await?;

        Ok(())
    }

    async fn gather_context(&mut self) -> Result<(), AppError> {
        let context_results = self
            .context_orchestrator
            .gather_context(
                &self.conversations,
                &self.current_objective.as_ref().map(|o| o.objective.clone()),
            )
            .await?;

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

        Ok(())
    }

    async fn execute_pending_tasks(&mut self) -> Result<(), AppError> {
        if self.executable_tasks.is_empty() {
            self.executable_tasks = self.get_executable_tasks();
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
                    success: true,
                    result: value,
                    error: None,
                },
                Err(e) => TaskExecutionResult {
                    task_id: task.id.clone(),
                    success: false,
                    result: json!(null),
                    error: Some(e.to_string()),
                },
            };

            self.task_results.insert(task.id.clone(), task_result);
        }

        Ok(())
    }

    async fn execute_single_task(&mut self, task: &ExecutableTask) -> Result<Value, AppError> {
        let context = json!({
            "task": task,
            "conversation_history": self.get_conversation_history_for_llm(),
            "task_results": self.task_results,
        });

        let request_body = render_template_with_prompt(AgentTemplates::TASK_EXECUTION, &context)
            .map_err(|e| {
                AppError::Internal(format!("Failed to render task execution template: {}", e))
            })?;

        let execution = self
            .create_main_llm()?
            .generate_structured_content::<TaskExecutionResponse>(request_body)
            .await?;

        self.store_conversation(
            ConversationContent::AssistantTaskExecution {
                task_execution: serde_json::to_value(&execution.task_execution)?,
                execution_status: match execution.execution_status {
                    crate::agentic::ExecutionStatus::Ready => "ready".to_string(),
                    crate::agentic::ExecutionStatus::Blocked => "blocked".to_string(),
                    crate::agentic::ExecutionStatus::CannotExecute => "cannot_execute".to_string(),
                },
                blocking_reason: execution.blocking_reason.clone(),
            },
            ConversationMessageType::AssistantTaskExecution,
        )
        .await?;

        if let Some(action) = execution.task_execution.actions.actions.first() {
            self.execute_action(action).await
        } else {
            Ok(json!({"error": "No action to execute"}))
        }
    }

    async fn execute_action(&self, action: &ExecutionAction) -> Result<Value, AppError> {
        match &action.action_type {
            TaskType::ToolCall => {
                let tool_call = self.parse_tool_call(&action.details)?;
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
                let workflow = self
                    .agent
                    .workflows
                    .iter()
                    .find(|w| w.name == workflow_call.workflow_name)
                    .ok_or_else(|| {
                        AppError::BadRequest(format!(
                            "Workflow '{}' not found",
                            workflow_call.workflow_name
                        ))
                    })?;
                self.workflow_executor
                    .execute_workflow(workflow, workflow_call.inputs, self.channel.clone())
                    .await
            }
            TaskType::KnowledgeSearch => {
                let query = self.extract_query_from_action(action)?;
                self.execute_search_action(&query, "knowledge").await
            }
            TaskType::ContextSearch => {
                let query = self.extract_query_from_action(action)?;
                self.execute_search_action(&query, "context").await
            }
        }
    }

    async fn execute_search_action(
        &self,
        query: &str,
        search_type: &str,
    ) -> Result<Value, AppError> {
        let results = match search_type {
            "knowledge" => {
                self.context_orchestrator
                    .execute_search_action("knowledge_base", query)
                    .await?
            }
            "context" => {
                self.context_orchestrator
                    .execute_search_action("all_sources", query)
                    .await?
            }
            _ => {
                self.context_orchestrator
                    .execute_search_action("all_sources", query)
                    .await?
            }
        };

        Ok(json!({
            "search_type": search_type,
            "query": query,
            "results": results,
            "result_count": results.len()
        }))
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

    fn parse_tool_call(&self, details: &Value) -> Result<shared::dto::json::ToolCall, AppError> {
        let tool_name = details["tool_name"]
            .as_str()
            .ok_or_else(|| AppError::BadRequest("Tool name not specified".to_string()))?;

        let tool = self
            .agent
            .tools
            .iter()
            .find(|t| t.name == tool_name)
            .ok_or_else(|| AppError::BadRequest(format!("Tool '{}' not found", tool_name)))?;

        let params = if let Some(params_value) = details.get("parameters") {
            self.validate_and_transform_parameters(params_value, tool)?
        } else {
            json!({})
        };

        Ok(shared::dto::json::ToolCall {
            tool_name: tool_name.to_string(),
            parameters: params,
        })
    }

    fn validate_and_transform_parameters(
        &self,
        params: &Value,
        tool: &AiTool,
    ) -> Result<Value, AppError> {
        match &tool.configuration {
            AiToolConfiguration::Api(api_config) => {
                if let Some(schema) = &api_config.request_body_schema {
                    let (properties, required) = Self::schema_fields_to_properties(schema);
                    self.validate_against_schema(params, &properties, &required)?;
                }
            }
            _ => {}
        }

        Ok(params.clone())
    }

    fn validate_against_schema(
        &self,
        params: &Value,
        properties: &Value,
        required: &[String],
    ) -> Result<(), AppError> {
        let params_obj = params
            .as_object()
            .ok_or_else(|| AppError::BadRequest("Parameters must be an object".to_string()))?;

        for field in required {
            if !params_obj.contains_key(field) {
                return Err(AppError::BadRequest(format!(
                    "Missing required parameter: {}",
                    field
                )));
            }
        }

        if let Some(props) = properties.as_object() {
            for (key, value) in params_obj {
                if let Some(prop_def) = props.get(key) {
                    let expected_type = prop_def["type"].as_str().unwrap_or("string");
                    self.validate_parameter_type(value, expected_type, key)?;
                }
            }
        }

        Ok(())
    }

    fn validate_parameter_type(
        &self,
        value: &Value,
        expected_type: &str,
        field_name: &str,
    ) -> Result<(), AppError> {
        let is_valid = match expected_type {
            "string" => value.is_string(),
            "number" => value.is_number(),
            "integer" => value.is_i64() || value.is_u64(),
            "boolean" => value.is_boolean(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            _ => true,
        };

        if !is_valid {
            return Err(AppError::BadRequest(format!(
                "Parameter '{}' should be of type '{}'",
                field_name, expected_type
            )));
        }

        Ok(())
    }

    fn parse_workflow_call(
        &self,
        details: &Value,
    ) -> Result<shared::dto::json::WorkflowCall, AppError> {
        serde_json::from_value(details.clone())
            .map_err(|e| AppError::BadRequest(format!("Invalid workflow call format: {}", e)))
    }

    fn extract_query_from_action(&self, action: &ExecutionAction) -> Result<String, AppError> {
        action
            .details
            .get("query")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| AppError::BadRequest("Search query not specified".to_string()))
    }

    async fn store_conversation(
        &mut self,
        content: ConversationContent,
        message_type: ConversationMessageType,
    ) -> Result<(), AppError> {
        let command =
            CreateConversationCommand::new(self.agent.id, self.context_id, content, message_type);
        let conversation = command.execute(&self.app_state).await?;
        self.conversations.push(conversation);
        Ok(())
    }

    async fn emit_event<T: serde::Serialize>(&self, data: &T) -> Result<(), AppError> {
        let event =
            StreamEvent::PlatformEvent("agent_event".to_string(), serde_json::to_value(data)?);
        self.channel
            .send(event)
            .await
            .map_err(|_| AppError::Internal("Failed to send event".to_string()))?;
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
            ConversationMessageType::AgentResponse
            | ConversationMessageType::AssistantAcknowledgment
            | ConversationMessageType::AssistantIdeation
            | ConversationMessageType::AssistantActionPlanning
            | ConversationMessageType::AssistantTaskExecution
            | ConversationMessageType::AssistantValidation => "assistant",
            ConversationMessageType::SystemDecision | ConversationMessageType::ContextResults => {
                "system"
            }
        }
    }

    fn extract_conversation_content(&self, content: &ConversationContent) -> String {
        serde_json::to_string(content).unwrap()
    }

    fn get_executable_tasks(&self) -> Vec<ExecutableTask> {
        self.conversations
            .iter()
            .filter_map(|conv| match &conv.content {
                ConversationContent::AssistantActionPlanning { task_execution, .. } => {
                    task_execution.get("tasks").and_then(|tasks| {
                        serde_json::from_value::<Vec<ExecutableTask>>(tasks.clone()).ok()
                    })
                }
                _ => None,
            })
            .flatten()
            .collect()
    }

    fn create_main_llm(&self) -> Result<GeminiClient, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_else(|_| "test-key".to_string());
        Ok(GeminiClient::new(
            api_key,
            Some("gemini-2.0-flash-exp".to_string()),
        ))
    }
}
