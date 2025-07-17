use crate::agentic::{
    ContextSearchDerivation, DecayManager, ExecutableTask, ExecutionAction, ExecutionStatus,
    LoopDecision, ParameterGenerationResponse, SearchScope, SharedExecutionContext,
    TaskBreakdownResponse, TaskExecutionResponse, TaskType, ToolExecutor, ValidationResponse,
    WorkflowExecutor, gemini_client::GeminiClient,
};
use crate::template::{AgentTemplates, render_template};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared::commands::{Command, CreateConversationCommand};
use shared::dto::json::StreamEvent;
use shared::error::AppError;
use shared::models::{
    AiAgentWithFeatures, AiTool, AiToolConfiguration, ContextAction, ContextEngineParams,
    ContextFilters, ContextSearchResult, ConversationContent, ConversationMessageType,
    ConversationRecord, MemoryRecordV2,
};
use shared::state::AppState;
use std::collections::HashMap;

const MAX_LOOP_ITERATIONS: usize = 50;
const DEFAULT_MIN_RELEVANCE: f64 = 0.7;
const DEFAULT_MAX_RESULTS: usize = 10;

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum NextStep {
    GatherContext,
    BreakdownTasks,
    ExecuteTasks,
    ValidateProgress,
    DeliverResponse,
    HandleError,
    Complete,
    DirectExecution,
}

impl ToString for NextStep {
    fn to_string(&self) -> String {
        match self {
            NextStep::GatherContext => "Gather Context".to_string(),
            NextStep::BreakdownTasks => "Breakdown Tasks".to_string(),
            NextStep::ExecuteTasks => "Execute Tasks".to_string(),
            NextStep::ValidateProgress => "Validate Progress".to_string(),
            NextStep::DeliverResponse => "Deliver Response".to_string(),
            NextStep::HandleError => "Handle Error".to_string(),
            NextStep::Complete => "Complete".to_string(),
            NextStep::DirectExecution => "Direct Execution".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StepDecision {
    pub next_step: NextStep,
    pub reasoning: String,
    pub confidence: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direct_exection_params: Option<DirectExecutionParams>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DirectExecutionParams {
    pub execution_type: String,
    pub resource_name: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename = "response")]
pub struct AcknowledgmentResponse {
    #[serde(rename = "message")]
    pub acknowledgment_message: String,
    pub further_action_required: bool,
    pub reasoning: String,
    pub objective: ObjectiveDefinition,
    pub conversation_insights: ConversationInsights,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ObjectiveDefinition {
    pub primary_goal: String,
    pub success_criteria: Vec<String>,
    pub constraints: Vec<String>,
    pub context_from_history: String,
    pub inferred_intent: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ConversationInsights {
    pub is_continuation: bool,
    pub topic_evolution: String,
    pub user_preferences: Vec<String>,
    pub relevant_past_outcomes: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TaskExecutionResult {
    pub task_id: String,
    pub task_name: String,
    pub task_type: String,
    pub status: String,
    pub success: bool,
    pub result: Option<Value>,
    pub error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct WorkflowValidationResponse {
    pub ready_to_execute: bool,
    pub missing_requirements: Vec<String>,
    pub validation_message: String,
}

pub struct AgentExecutor {
    pub agent: AiAgentWithFeatures,
    pub app_state: AppState,
    pub context_id: i64,
    pub conversations: Vec<ConversationRecord>,
    shared_context: SharedExecutionContext,
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
            app_state,
            context_id,
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

        Ok(AgentExecutor {
            agent: self.agent,
            app_state: self.app_state,
            context_id: self.context_id,
            shared_context,
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

    // ==================== LLM Management ====================
    fn get_gemini_api_key() -> Result<String, AppError> {
        std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))
    }

    pub fn create_strong_llm(&self) -> Result<GeminiClient, AppError> {
        let api_key = Self::get_gemini_api_key()?;
        Ok(GeminiClient::new(
            api_key,
            Some("gemini-2.5-pro".to_string()),
        ))
    }

    pub fn create_weak_llm(&self) -> Result<GeminiClient, AppError> {
        let api_key = Self::get_gemini_api_key()?;
        Ok(GeminiClient::new(
            api_key,
            Some("gemini-2.5-flash".to_string()),
        ))
    }

    fn create_agent_response_content(&self, message: String) -> ConversationContent {
        ConversationContent::AgentResponse {
            response: message,
            citations: Vec::new(),
            context_used: Vec::new(),
        }
    }

    async fn store_conversation(
        &mut self,
        typed_content: shared::models::ConversationContent,
        message_type: shared::models::ConversationMessageType,
    ) -> Result<(), AppError> {
        let start = Utc::now();

        let message = CreateConversationCommand::new(
            self.app_state.sf.next_id()? as i64,
            self.context_id,
            typed_content.clone(),
            message_type.clone(),
        )
        .execute(&self.app_state)
        .await?;
        self.conversations.push(message.clone());

        let db_time = Utc::now();
        
        // Add timestamp to the message for tracking
        let send_timestamp = Utc::now();
        println!("Sending message to channel at: {}", send_timestamp);
        
        println!("Channel state before send - capacity: {}, len: {}", 
            self.channel.capacity(), 
            self.channel.max_capacity() - self.channel.capacity()
        );
        
        let send_result = self
            .channel
            .send(StreamEvent::ConversationMessage(message))
            .await;
            
        if let Err(e) = send_result {
            println!("Channel send error: {:?}", e);
        } else {
            println!("Message successfully sent to channel");
        }

        let channel_time = Utc::now();
        
        println!("store_conversation timings - DB: {}ms, Channel send: {}ms, Total: {}ms", 
            (db_time - start).num_milliseconds(),
            (channel_time - db_time).num_milliseconds(),
            (channel_time - start).num_milliseconds()
        );

        Ok(())
    }

    fn get_conversation_history_for_llm(&self) -> Vec<Value> {
        self.conversations
            .iter()
            .filter_map(|msg| {
                let role = match msg.message_type {
                    ConversationMessageType::UserMessage => "user",
                    _ => "model",
                };

                serde_json::to_string(&msg.content).ok().map(|content| {
                    json!({
                        "role": role,
                        "content": content
                    })
                })
            })
            .collect()
    }

    pub async fn execute_with_streaming(&mut self, user_message: &str) -> Result<(), AppError> {
        self.load_immediate_context().await?;
        self.user_request = user_message.to_string();

        let user_content = ConversationContent::UserMessage {
            message: user_message.to_string(),
        };
        self.store_conversation(user_content, ConversationMessageType::UserMessage)
            .await?;

        let acknowledgment_response = match self.generate_acknowledgment().await {
            Ok(response) => response,
            Err(e) => {
                let error_content = ConversationContent::AssistantValidation {
                    validation_result: json!({
                        "error": e.to_string(),
                        "error_type": "acknowledgment_generation_failed"
                    }),
                    loop_decision: "abort_unresolvable".to_string(),
                    decision_reasoning: format!("Failed to generate acknowledgment: {}", e),
                    next_iteration_focus: None,
                    has_unresolvable_errors: true,
                    unresolvable_error_details: Some(e.to_string()),
                };
                self.store_conversation(
                    error_content,
                    ConversationMessageType::AssistantValidation,
                )
                .await?;
                return Err(e);
            }
        };

        if !acknowledgment_response.further_action_required {
            return Ok(());
        }

        // Main decision loop
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

                NextStep::ValidateProgress => {
                    self.execute_validation_step().await?;
                }

                NextStep::DeliverResponse => {
                    self.generate_and_send_summary().await?;
                    return Ok(());
                }

                NextStep::HandleError => {
                    let handled = self.handle_errors().await?;
                    if !handled {
                        self.generate_and_send_summary().await?;
                        return Ok(());
                    }
                }

                NextStep::Complete => {
                    return Ok(());
                }

                NextStep::DirectExecution => {
                    if let Some(tool_info) = decision.direct_exection_params {
                        self.execute_direct_tool_call(tool_info).await?;
                    }
                }
            }
        }
    }

    async fn load_immediate_context(&mut self) -> Result<(), AppError> {
        let immediate_context = self
            .decay_manager
            .get_immediate_context(self.context_id)
            .await?;

        self.memories = immediate_context.memories;
        self.conversations = immediate_context.conversations;
        Ok(())
    }

    async fn decide_next_step(&mut self) -> Result<StepDecision, AppError> {
        let decision_context = json!({
            "conversation_history": self.get_conversation_history_for_llm(),
            "current_objective": self.current_objective,
            "conversation_insights": self.conversation_insights,
            "available_tools": self.agent.tools,
            "available_workflows": self.agent.workflows,
            "available_knowledge_bases": self.agent.knowledge_bases,
            "iteration_info": {
                "current_iteration": self.get_current_iteration(),
                "max_iterations": MAX_LOOP_ITERATIONS,
            }
        });

        let prompt =
            render_template(AgentTemplates::STEP_DECISION, &decision_context).map_err(|e| {
                AppError::Internal(format!("Failed to render step decision template: {}", e))
            })?;

        let (_, decision) = self
            .create_strong_llm()?
            .generate_structured_content::<StepDecision>(prompt)
            .await?;

        self.conversations.push(ConversationRecord {
            id: self.app_state.sf.next_id()? as i64,
            context_id: self.context_id,
            timestamp: Utc::now(),
            content: ConversationContent::SystemDecision {
                step: decision.next_step.to_string(),
                reasoning: decision.reasoning.clone(),
                confidence: decision.confidence,
            },
            message_type: ConversationMessageType::SystemDecision,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });

        Ok(decision)
    }

    async fn generate_acknowledgment(&mut self) -> Result<AcknowledgmentResponse, AppError> {
        let acknowledgment_context = self.build_acknowledgment_context();

        let request_body = render_template(AgentTemplates::ACKNOWLEDGMENT, &acknowledgment_context)
            .map_err(|e| {
                AppError::Internal(format!("Failed to render acknowledgment template: {}", e))
            })?;

        let (_, parsed) = self
            .create_weak_llm()?
            .generate_structured_content::<AcknowledgmentResponse>(request_body)
            .await?;

        self.current_objective = Some(parsed.objective.clone());
        self.conversation_insights = Some(parsed.conversation_insights.clone());

        self.store_acknowledgment_conversation(&parsed).await?;

        Ok(parsed)
    }

    fn build_acknowledgment_context(&self) -> Value {
        json!({
            "tools": &self.agent.tools,
            "workflows": &self.agent.workflows,
            "knowledge_bases": &self.agent.knowledge_bases,
            "conversation_history": self.get_conversation_history_for_llm(),
            "memories": self.memories.iter().map(|m| &m.content).collect::<Vec<_>>(),
        })
    }

    async fn store_acknowledgment_conversation(
        &mut self,
        acknowledgment: &AcknowledgmentResponse,
    ) -> Result<(), AppError> {
        let acknowledgment_content = ConversationContent::AssistantAcknowledgment {
            acknowledgment_message: acknowledgment.acknowledgment_message.clone(),
            further_action_required: acknowledgment.further_action_required,
            reasoning: acknowledgment.reasoning.clone(),
        };

        self.store_conversation(
            acknowledgment_content,
            ConversationMessageType::AssistantAcknowledgment,
        )
        .await?;

        Ok(())
    }

    async fn gather_context(&mut self) -> Result<(), AppError> {
        let search_params = self.derive_context_search_parameters().await?;

        let context_results = match search_params.search_scope {
            SearchScope::KnowledgeBase => {
                self.search_knowledge_bases_with_filters(&search_params)
                    .await?
            }
            SearchScope::Memory => self.search_memories_with_filters(&search_params).await?,
            SearchScope::AllSources => {
                let mut results = self
                    .search_knowledge_bases_with_filters(&search_params)
                    .await?;
                let memory_results = self.search_memories_with_filters(&search_params).await?;
                results.extend(memory_results);
                results
            }
        };

        let mut final_results = context_results;
        if final_results.is_empty() && !search_params.alternative_queries.is_empty() {
            for alt_query in &search_params.alternative_queries {
                let mut alt_params = search_params.clone();
                alt_params.search_query = alt_query.clone();

                let alt_results = match search_params.search_scope {
                    SearchScope::KnowledgeBase => {
                        self.search_knowledge_bases_with_filters(&alt_params)
                            .await?
                    }
                    SearchScope::Memory => self.search_memories_with_filters(&alt_params).await?,
                    SearchScope::AllSources => {
                        let mut results = self
                            .search_knowledge_bases_with_filters(&alt_params)
                            .await?;
                        let memory_results = self.search_memories_with_filters(&alt_params).await?;
                        results.extend(memory_results);
                        results
                    }
                };

                if !alt_results.is_empty() {
                    final_results = alt_results;
                    break;
                }
            }
        }

        self.store_conversation(
            ConversationContent::ContextResults {
                query: search_params.search_query.clone(),
                results: serde_json::to_value(&final_results)?,
                result_count: final_results.len(),
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

        if self.executable_tasks.is_empty() {
            return Ok(());
        }

        let results = self.execute_tasks(&self.executable_tasks.clone()).await?;

        for result in results {
            self.task_results.insert(result.task_id.clone(), result);
        }

        Ok(())
    }

    async fn handle_errors(&mut self) -> Result<bool, AppError> {
        let errors = self.get_recent_errors();

        if errors.is_empty() {
            return Ok(true);
        }

        let unresolvable_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.contains("unresolvable") || e.contains("cannot_execute"))
            .collect();

        if !unresolvable_errors.is_empty() {
            self.store_conversation(
                ConversationContent::AssistantValidation {
                    validation_result: json!({
                        "error_summary": unresolvable_errors,
                        "error_type": "unresolvable_errors"
                    }),
                    loop_decision: "abort_unresolvable".to_string(),
                    decision_reasoning:
                        "Encountered unresolvable errors that prevent further progress".to_string(),
                    next_iteration_focus: None,
                    has_unresolvable_errors: true,
                    unresolvable_error_details: Some(
                        unresolvable_errors
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join("; "),
                    ),
                },
                ConversationMessageType::AssistantValidation,
            )
            .await?;

            return Ok(false);
        }

        Ok(true)
    }

    async fn execute_direct_tool_call(
        &mut self,
        tool_info: DirectExecutionParams,
    ) -> Result<(), AppError> {
        let action = ExecutionAction {
            action_type: if tool_info.execution_type == "tool" {
                TaskType::ToolCall
            } else {
                TaskType::WorkflowCall
            },
            details: json!({
                "resource_name": tool_info.resource_name
            }),
            purpose: "Direct tool execution for simple request".to_string(),
        };

        let task = ExecutableTask {
            id: "direct_tool_call".to_string(),
            name: format!("Execute {}", tool_info.resource_name),
            description: format!("Direct execution of {} tool", tool_info.resource_name),
            dependencies: vec![],
            success_criteria: "Tool executes successfully and returns result".to_string(),
            error_handling: "Report error if tool execution fails".to_string(),
            can_run_parallel: false,
        };

        let result = self.execute_action(&action, &task).await?;

        let action_result_content = ConversationContent::AssistantTaskExecution {
            task_execution: json!({
                "action": action,
                "result": result
            }),
            execution_status: "completed".to_string(),
            blocking_reason: None,
        };

        self.store_conversation(
            action_result_content,
            ConversationMessageType::AssistantTaskExecution,
        )
        .await?;

        Ok(())
    }

    async fn generate_and_send_summary(&mut self) -> Result<(), AppError> {
        let summary_context = json!({
            "conversation_history": self.get_conversation_history_for_llm(),
            "current_objective": self.current_objective,
            "conversation_insights": self.conversation_insights,
        });

        let request_body = render_template(AgentTemplates::SUMMARY, &summary_context)
            .map_err(|e| AppError::Internal(format!("Failed to render summary template: {}", e)))?;

        // Generate the response
        #[derive(Deserialize, Serialize)]
        struct SummaryResponse {
            response: String,
        }

        let (_, summary) = self
            .create_weak_llm()?
            .generate_structured_content::<SummaryResponse>(request_body)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to generate summary: {}", e)))?;

        // Send as agent response
        self.store_conversation(
            self.create_agent_response_content(summary.response),
            ConversationMessageType::AgentResponse,
        )
        .await?;

        Ok(())
    }

    // ==================== Task Execution Methods ====================

    async fn execute_task_breakdown_step(&mut self) -> Result<TaskBreakdownResponse, AppError> {
        let task_breakdown_context = json!({
            "conversation_history": self.get_conversation_history_for_llm(),
            "current_objective": self.current_objective,
            "conversation_insights": self.conversation_insights,
        });

        let request_body = render_template(AgentTemplates::TASK_BREAKDOWN, &task_breakdown_context)
            .map_err(|e| {
                AppError::Internal(format!("Failed to render task breakdown template: {}", e))
            })?;

        let (_, parsed) = self
            .create_strong_llm()?
            .generate_structured_content::<TaskBreakdownResponse>(request_body)
            .await?;

        // Store task breakdown as a task execution
        let task_content = ConversationContent::AssistantTaskExecution {
            task_execution: json!({
                "approach": "Breaking down tasks for execution",
                "actions": {
                    "action": parsed.tasks.iter().map(|task| json!({
                        "type": "tool_call",
                        "details": task,
                        "purpose": task.description.clone()
                    })).collect::<Vec<_>>()
                },
                "expected_result": "Task breakdown completed"
            }),
            execution_status: "ready".to_string(),
            blocking_reason: None,
        };

        self.store_conversation(
            task_content,
            ConversationMessageType::AssistantTaskExecution,
        )
        .await?;

        // Store the tasks in the executor
        self.executable_tasks = parsed.tasks.clone();

        Ok(parsed)
    }

    async fn execute_tasks(
        &mut self,
        tasks: &[ExecutableTask],
    ) -> Result<Vec<TaskExecutionResult>, AppError> {
        let mut results = Vec::new();
        let mut completed_tasks = HashMap::new();

        for task in tasks {
            let task_result = if self.are_dependencies_met(task, &completed_tasks) {
                self.execute_single_task(task).await?
            } else {
                self.create_skipped_task_result(task, "Dependencies not met")
            };

            completed_tasks.insert(task.id.clone(), task_result.clone());
            results.push(task_result);
        }

        Ok(results)
    }

    // Helper: Check if task dependencies are met
    fn are_dependencies_met(
        &self,
        task: &ExecutableTask,
        completed_tasks: &HashMap<String, TaskExecutionResult>,
    ) -> bool {
        task.dependencies
            .iter()
            .all(|dep_id| completed_tasks.get(dep_id).map_or(false, |r| r.success))
    }

    // Helper: Create a skipped task result
    fn create_skipped_task_result(
        &self,
        task: &ExecutableTask,
        reason: &str,
    ) -> TaskExecutionResult {
        TaskExecutionResult {
            task_id: task.id.clone(),
            task_name: task.name.clone(),
            task_type: "task".to_string(),
            status: "skipped".to_string(),
            success: false,
            result: None,
            error: Some(reason.to_string()),
        }
    }

    async fn execute_single_task(
        &mut self,
        task: &ExecutableTask,
    ) -> Result<TaskExecutionResult, AppError> {
        let task_execution_context = json!({
            "current_task": task,
            "conversation_history": self.get_conversation_history_for_llm(),
            "current_objective": self.current_objective,
            "conversation_insights": self.conversation_insights,
        });

        let request_body = render_template(AgentTemplates::TASK_EXECUTION, &task_execution_context)
            .map_err(|e| {
                AppError::Internal(format!("Failed to render task execution template: {}", e))
            })?;

        let (_, parsed) = self
            .create_weak_llm()?
            .generate_structured_content::<TaskExecutionResponse>(request_body)
            .await?;

        // Store action planning response with typed content
        let task_content = ConversationContent::AssistantActionPlanning {
            task_execution: serde_json::to_value(&parsed.task_execution).unwrap_or_default(),
            execution_status: match &parsed.execution_status {
                ExecutionStatus::Ready => "ready",
                ExecutionStatus::Blocked => "blocked",
                ExecutionStatus::CannotExecute => "cannot_execute",
            }
            .to_string(),
            blocking_reason: parsed.blocking_reason.clone(),
        };

        self.store_conversation(
            task_content,
            ConversationMessageType::AssistantActionPlanning,
        )
        .await?;

        let mut task_results = Vec::new();

        if matches!(parsed.execution_status, ExecutionStatus::Ready) {
            for action in &parsed.task_execution.actions.actions {
                match self.execute_action(action, task).await {
                    Ok(action_result) => {
                        let action_result_content = ConversationContent::AssistantTaskExecution {
                            task_execution: json!({
                                "action": action,
                                "result": action_result
                            }),
                            execution_status: "ready".to_string(),
                            blocking_reason: None,
                        };

                        self.store_conversation(
                            action_result_content,
                            ConversationMessageType::AssistantTaskExecution,
                        )
                        .await?;
                        task_results.push(action_result);
                    }
                    Err(e) => {
                        return Ok(TaskExecutionResult {
                            task_id: task.id.clone(),
                            task_name: task.name.clone(),
                            task_type: "task".to_string(),
                            status: "failed".to_string(),
                            success: false,
                            result: None,
                            error: Some(format!("Action execution failed: {}", e)),
                        });
                    }
                }
            }
        }

        Ok(TaskExecutionResult {
            task_id: task.id.clone(),
            task_name: task.name.clone(),
            task_type: "task".to_string(),
            status: if matches!(parsed.execution_status, ExecutionStatus::Ready) {
                "completed"
            } else {
                "blocked"
            }
            .to_string(),
            success: matches!(parsed.execution_status, ExecutionStatus::Ready),
            result: Some(json!({
                "direction": parsed,
                "action_results": task_results
            })),
            error: if !matches!(parsed.execution_status, ExecutionStatus::Ready) {
                parsed.blocking_reason
            } else {
                None
            },
        })
    }

    // ==================== Validation Methods ====================

    async fn execute_validation_step(&mut self) -> Result<ValidationResponse, AppError> {
        let validation_context = json!({
            "conversation_history": self.get_conversation_history_for_llm(),
            "current_objective": self.current_objective,
            "conversation_insights": self.conversation_insights,
            "executed_tasks": self.executable_tasks,
            "task_results": self.task_results,
        });

        let request_body = render_template(AgentTemplates::VALIDATION, &validation_context)
            .map_err(|e| {
                AppError::Internal(format!("Failed to render validation template: {}", e))
            })?;

        let (_, parsed) = self
            .create_weak_llm()?
            .generate_structured_content::<ValidationResponse>(request_body)
            .await?;

        // Store validation response with typed content
        let validation_content = ConversationContent::AssistantValidation {
            validation_result: serde_json::to_value(&parsed.validation_result).unwrap_or_default(),
            loop_decision: match &parsed.loop_decision {
                LoopDecision::Continue => "continue",
                LoopDecision::Complete => "complete",
                LoopDecision::AbortUnresolvable => "abort_unresolvable",
            }
            .to_string(),
            decision_reasoning: parsed.decision_reasoning.clone(),
            next_iteration_focus: parsed.next_iteration_focus.clone(),
            has_unresolvable_errors: parsed.has_unresolvable_errors,
            unresolvable_error_details: parsed.unresolvable_error_details.clone(),
        };

        self.store_conversation(
            validation_content,
            ConversationMessageType::AssistantValidation,
        )
        .await?;

        Ok(parsed)
    }

    // ==================== Context Search Methods ====================

    async fn search_context(&self, query: &str) -> Result<Vec<ContextSearchResult>, AppError> {
        let params = ContextEngineParams {
            query: query.to_string(),
            action: ContextAction::SearchAll,
            filters: Some(ContextFilters {
                max_results: DEFAULT_MAX_RESULTS,
                min_relevance: DEFAULT_MIN_RELEVANCE,
                time_range: None,
                search_mode: shared::models::SearchMode::default(),
                boost_keywords: None,
            }),
        };

        self.shared_context.context_engine().execute(params).await
    }

    async fn search_knowledge_bases(
        &self,
        query: &str,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let params = ContextEngineParams {
            query: query.to_string(),
            action: ContextAction::SearchKnowledgeBase { kb_id: None }, // None means search all agent's KBs
            filters: Some(ContextFilters {
                max_results: DEFAULT_MAX_RESULTS,
                min_relevance: DEFAULT_MIN_RELEVANCE,
                time_range: None,
                search_mode: shared::models::SearchMode::default(),
                boost_keywords: None,
            }),
        };

        self.shared_context.context_engine().execute(params).await
    }

    // ==================== Action Execution Methods ====================

    // Helper: Extract resource name from action details
    fn extract_resource_name<'a>(
        &self,
        action: &'a ExecutionAction,
        resource_type: &str,
    ) -> Result<&'a str, AppError> {
        action
            .details
            .get("resource_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::Internal(format!(
                    "{} name not found in action details",
                    resource_type
                ))
            })
    }

    // Helper: Find tool by name
    fn find_tool_by_name(&self, tool_name: &str) -> Result<&AiTool, AppError> {
        self.agent
            .tools
            .iter()
            .find(|t| t.name == tool_name)
            .ok_or_else(|| AppError::NotFound(format!("Tool {} not found", tool_name)))
    }

    // Helper: Find workflow by name
    fn find_workflow_by_name(
        &self,
        workflow_name: &str,
    ) -> Result<&shared::models::AiWorkflow, AppError> {
        self.agent
            .workflows
            .iter()
            .find(|w| w.name == workflow_name)
            .ok_or_else(|| AppError::NotFound(format!("Workflow {} not found", workflow_name)))
    }

    // Helper: Extract query from action
    fn extract_query_from_action<'a>(
        &self,
        action: &'a ExecutionAction,
    ) -> Result<&'a str, AppError> {
        action
            .details
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Internal("Query not found in action details".to_string()))
    }

    async fn execute_action(
        &mut self,
        action: &ExecutionAction,
        task: &ExecutableTask,
    ) -> Result<Value, AppError> {
        match &action.action_type {
            TaskType::ToolCall => {
                let tool_name = self.extract_resource_name(action, "Tool")?;
                let tool = self.find_tool_by_name(tool_name)?;

                let parameters = self
                    .generate_parameters_for_tool(tool, action, task)
                    .await?;

                self.tool_executor
                    .execute_tool_immediately(tool, parameters)
                    .await
            }
            TaskType::WorkflowCall => {
                let workflow_name = self.extract_resource_name(action, "Workflow")?;
                let workflow = self.find_workflow_by_name(workflow_name)?.clone();

                // Prepare workflow call inputs from action details
                let inputs = action
                    .details
                    .get("inputs")
                    .cloned()
                    .unwrap_or_else(|| json!({}));

                let workflow_call = shared::dto::json::WorkflowCall {
                    workflow_name: workflow_name.to_string(),
                    inputs,
                };

                // Execute workflow with context gathering loop (following acknowledgment flow pattern)
                self.execute_workflow_with_context_gathering(workflow_call, &workflow)
                    .await
            }
            TaskType::KnowledgeSearch => {
                let query = self.extract_query_from_action(action)?;
                self.execute_search_action(query, "knowledge").await
            }
            TaskType::ContextSearch => {
                let query = self.extract_query_from_action(action)?;
                self.execute_search_action(query, "context").await
            }
        }
    }

    // ==================== Workflow Methods ====================

    async fn execute_workflow_with_context_gathering(
        &mut self,
        workflow_call: shared::dto::json::WorkflowCall,
        workflow: &shared::models::AiWorkflow,
    ) -> Result<Value, AppError> {
        // Step 1: Validate workflow conditions
        let validation_response = self
            .validate_workflow_conditions(&workflow_call, workflow)
            .await?;

        if validation_response.ready_to_execute {
            // Workflow is ready - execute directly
            let result = self
                .workflow_executor
                .execute_workflow_task(
                    &workflow_call,
                    &self.agent.workflows,
                    &[],
                    self.channel.clone(),
                )
                .await?;

            return Ok(json!({
                "workflow_name": workflow_call.workflow_name,
                "status": "completed",
                "result": result
            }));
        }

        // Step 2: Single-pass context gathering for missing requirements
        if validation_response.missing_requirements.is_empty() {
            return Err(AppError::BadRequest(format!(
                "Workflow validation failed but no specific requirements identified: {}",
                validation_response.validation_message
            )));
        }

        // Gather context for each missing requirement
        let mut gathered_context = json!({});
        let mut context_found = false;

        for requirement in &validation_response.missing_requirements {
            match self.search_context(requirement).await {
                Ok(search_results) => {
                    if !search_results.is_empty() {
                        let relevant_data = search_results
                            .into_iter()
                            .map(|r| r.content)
                            .collect::<Vec<_>>()
                            .join(" ");

                        let context_key = requirement
                            .to_lowercase()
                            .replace(" ", "_")
                            .replace("-", "_");
                        gathered_context[context_key] = json!(relevant_data);
                        context_found = true;
                    }
                }
                Err(_) => {
                    // Continue with other requirements even if one fails
                }
            }
        }

        // Step 3: Execute workflow with enhanced context or fail if no context found
        if context_found {
            let mut enhanced_inputs = workflow_call.inputs.clone();
            if let Some(enhanced_obj) = enhanced_inputs.as_object_mut() {
                for (key, value) in gathered_context.as_object().unwrap() {
                    enhanced_obj.insert(key.clone(), value.clone());
                }
            }

            let enhanced_workflow_call = shared::dto::json::WorkflowCall {
                workflow_name: workflow_call.workflow_name.clone(),
                inputs: enhanced_inputs,
            };

            let result = self
                .workflow_executor
                .execute_workflow_task(
                    &enhanced_workflow_call,
                    &self.agent.workflows,
                    &[],
                    self.channel.clone(),
                )
                .await?;

            Ok(json!({
                "workflow_name": workflow_call.workflow_name,
                "status": "completed_with_context_gathering",
                "result": result,
                "gathered_context": gathered_context
            }))
        } else {
            Err(AppError::BadRequest(format!(
                "Unable to gather required context for workflow. Missing: {}",
                validation_response.missing_requirements.join(", ")
            )))
        }
    }

    async fn validate_workflow_conditions(
        &self,
        workflow_call: &shared::dto::json::WorkflowCall,
        workflow: &shared::models::AiWorkflow,
    ) -> Result<WorkflowValidationResponse, AppError> {
        // Find the trigger node to analyze its conditions
        let trigger_node = workflow
            .workflow_definition
            .nodes
            .iter()
            .find(|node| matches!(node.node_type, shared::models::WorkflowNodeType::Trigger(_)))
            .ok_or_else(|| AppError::BadRequest("No trigger node found in workflow".to_string()))?;

        let trigger_config =
            if let shared::models::WorkflowNodeType::Trigger(config) = &trigger_node.node_type {
                config
            } else {
                return Err(AppError::Internal("Invalid trigger node type".to_string()));
            };

        // Create validation context
        let validation_context = json!({
            "workflow_name": workflow.name,
            "workflow_description": workflow.description,
            "trigger_description": trigger_config.description,
            "trigger_condition": trigger_config.condition,
            "current_inputs": workflow_call.inputs,
            "available_data": workflow_call.inputs.as_object().map(|obj| obj.keys().collect::<Vec<_>>()).unwrap_or_default(),
        });

        // Use template system for validation prompt
        let request_body = render_template(
            crate::template::AgentTemplates::WORKFLOW_VALIDATION,
            &validation_context,
        )
        .map_err(|e| AppError::Internal(format!("Template rendering failed: {}", e)))?;

        // Use LLM for intelligent validation with structured generation
        let (_, validation_result) = self
            .create_weak_llm()?
            .generate_structured_content::<WorkflowValidationResponse>(request_body)
            .await?;

        Ok(validation_result)
    }

    // Helper: Execute search action
    async fn execute_search_action(
        &self,
        query: &str,
        search_type: &str,
    ) -> Result<Value, AppError> {
        println!("here {:?} {}", query, search_type);

        let results = match search_type {
            "knowledge" => self.search_knowledge_bases(query).await?,
            "context" => self.search_context(query).await?,
            _ => self.search_context(query).await?,
        };

        Ok(json!({
            "search_type": search_type,
            "query": query,
            "results": results,
            "result_count": results.len()
        }))
    }

    // ==================== Schema and Parameter Methods ====================

    fn schema_fields_to_properties(fields: &[shared::models::SchemaField]) -> (Value, Vec<String>) {
        let mut properties = json!({});
        let mut required = Vec::new();

        for field in fields {
            properties[&field.name] = json!({
                "type": field.field_type.to_uppercase(),
                "description": field.description.as_deref().unwrap_or("")
            });

            if field.required {
                required.push(field.name.clone());
            }
        }

        (properties, required)
    }

    fn build_parameter_schema(tool: &AiTool) -> Value {
        match &tool.configuration {
            AiToolConfiguration::Api(config) => {
                let mut properties = json!({
                    "generation_required": {
                        "type": "BOOLEAN",
                        "description": "Whether parameter generation was required for this tool"
                    }
                });
                let mut required = vec!["generation_required".to_string()];

                if let Some(url_schema) = &config.url_params_schema {
                    if !url_schema.is_empty() {
                        let (url_props, url_required) =
                            Self::schema_fields_to_properties(url_schema);
                        properties["url_params"] = json!({
                            "type": "OBJECT",
                            "properties": url_props,
                            "required": url_required
                        });
                        required.push("url_params".to_string());
                    }
                }

                if let Some(query_schema) = &config.query_params_schema {
                    if !query_schema.is_empty() {
                        let (query_props, query_required) =
                            Self::schema_fields_to_properties(query_schema);
                        properties["query_params"] = json!({
                            "type": "OBJECT",
                            "properties": query_props,
                            "required": query_required
                        });
                        required.push("query_params".to_string());
                    }
                }

                if let Some(body_schema) = &config.request_body_schema {
                    if !body_schema.is_empty() {
                        let (body_props, body_required) =
                            Self::schema_fields_to_properties(body_schema);
                        properties["body"] = json!({
                            "type": "OBJECT",
                            "properties": body_props,
                            "required": body_required
                        });
                        required.push("body".to_string());
                    }
                }

                json!({
                    "type": "OBJECT",
                    "properties": properties,
                    "required": required
                })
            }
            AiToolConfiguration::KnowledgeBase(_) => {
                json!({
                    "type": "OBJECT",
                    "properties": {
                        "generation_required": {
                            "type": "BOOLEAN",
                            "description": "Whether parameter generation was required for this tool"
                        },
                        "query": {
                            "type": "STRING",
                            "description": "Search query"
                        }
                    },
                    "required": ["generation_required", "query"]
                })
            }
            AiToolConfiguration::PlatformEvent(_) => {
                json!({
                    "type": "OBJECT",
                    "properties": {
                        "generation_required": {
                            "type": "BOOLEAN",
                            "description": "Whether parameter generation was required for this tool"
                        },
                        "event_data": {
                            "type": "OBJECT",
                            "description": "Event data",
                            "properties": {
                                "data": {
                                    "type": "OBJECT",
                                    "description": "Event payload"
                                }
                            }
                        }
                    },
                    "required": ["generation_required"]
                })
            }
            AiToolConfiguration::PlatformFunction(config) => {
                let mut properties = json!({
                    "generation_required": {
                        "type": "BOOLEAN",
                        "description": "Whether parameter generation was required for this tool"
                    }
                });
                let mut required = vec!["generation_required".to_string()];

                if let Some(input_schema) = &config.input_schema {
                    let (schema_props, schema_required) =
                        Self::schema_fields_to_properties(input_schema);
                    for (key, value) in schema_props.as_object().unwrap() {
                        properties[key] = value.clone();
                    }
                    required.extend(schema_required);
                }

                json!({
                    "type": "OBJECT",
                    "properties": properties,
                    "required": required
                })
            }
        }
    }

    async fn generate_parameters_for_tool(
        &self,
        tool: &AiTool,
        action: &ExecutionAction,
        task: &ExecutableTask,
    ) -> Result<Value, AppError> {
        let parameter_schema = Self::build_parameter_schema(tool);

        let param_gen_context = json!({
            "action": action,
            "tool_config": tool,
            "task": task,
            "parameter_schema": parameter_schema,
            "conversation_history": self.get_conversation_history_for_llm(),
        });

        let request_body =
            render_template(AgentTemplates::PARAMETER_GENERATION, &param_gen_context).map_err(
                |e| {
                    AppError::Internal(format!(
                        "Failed to render parameter generation template: {}",
                        e
                    ))
                },
            )?;

        let (_, parsed) = self
            .create_weak_llm()?
            .generate_structured_content::<ParameterGenerationResponse>(request_body)
            .await?;

        if !parsed.parameter_generation.can_generate {
            return Err(AppError::Internal(format!(
                "Cannot generate parameters: {:?}",
                parsed.parameter_generation.missing_information
            )));
        }

        Ok(parsed.parameter_generation.parameters)
    }

    fn get_current_iteration(&self) -> usize {
        self.conversations
            .iter()
            .filter(|msg| matches!(&msg.message_type, ConversationMessageType::SystemDecision))
            .count()
    }

    fn get_executable_tasks(&self) -> Vec<ExecutableTask> {
        self.conversations
            .iter()
            .rev()
            .find_map(|msg| match &msg.content {
                ConversationContent::AssistantTaskExecution { task_execution, .. } => {
                    task_execution
                        .get("actions")
                        .and_then(|a| a.get("action"))
                        .and_then(|actions| actions.as_array())
                        .map(|actions| {
                            actions
                                .iter()
                                .filter_map(|a| serde_json::from_value(a.clone()).ok())
                                .collect()
                        })
                }
                _ => None,
            })
            .unwrap_or_default()
    }

    fn get_recent_errors(&self) -> Vec<String> {
        self.conversations
            .iter()
            .filter_map(|msg| match &msg.content {
                ConversationContent::AssistantValidation {
                    unresolvable_error_details,
                    has_unresolvable_errors,
                    ..
                } => {
                    if *has_unresolvable_errors {
                        unresolvable_error_details.clone()
                    } else {
                        None
                    }
                }
                ConversationContent::AssistantTaskExecution {
                    blocking_reason,
                    execution_status,
                    ..
                } => {
                    if execution_status == "blocked" || execution_status == "cannot_execute" {
                        blocking_reason.clone()
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect()
    }

    async fn derive_context_search_parameters(&self) -> Result<ContextSearchDerivation, AppError> {
        let search_context = json!({
            "conversation_history": self.get_conversation_history_for_llm(),
            "current_objective": self.current_objective,
            "conversation_insights": self.conversation_insights,
        });

        let request_body =
            render_template(AgentTemplates::CONTEXT_SEARCH_DERIVATION, &search_context).map_err(
                |e| AppError::Internal(format!("Failed to render context search template: {}", e)),
            )?;

        let (_, search_params) = self
            .create_weak_llm()?
            .generate_structured_content::<ContextSearchDerivation>(request_body)
            .await?;

        Ok(search_params)
    }

    async fn search_knowledge_bases_with_filters(
        &self,
        search_params: &ContextSearchDerivation,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let params = ContextEngineParams {
            query: search_params.search_query.clone(),
            action: ContextAction::SearchKnowledgeBase { kb_id: None },
            filters: Some(ContextFilters {
                max_results: search_params.filters.max_results as usize,
                min_relevance: search_params.filters.min_relevance,
                time_range: self.parse_time_range(&search_params.filters.time_range),
                search_mode: match &search_params.filters.search_mode {
                    crate::agentic::SearchModeType::Semantic => shared::models::SearchMode::Vector,
                    crate::agentic::SearchModeType::Keyword => shared::models::SearchMode::FullText,
                    crate::agentic::SearchModeType::Hybrid => shared::models::SearchMode::Hybrid {
                        vector_weight: 0.5,
                        text_weight: 0.5,
                    },
                },
                boost_keywords: search_params.filters.boost_keywords.clone(),
            }),
        };

        self.shared_context.context_engine().execute(params).await
    }

    async fn search_memories_with_filters(
        &self,
        search_params: &ContextSearchDerivation,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let params = ContextEngineParams {
            query: search_params.search_query.clone(),
            action: ContextAction::SearchMemories { category: None },
            filters: Some(ContextFilters {
                max_results: search_params.filters.max_results as usize,
                min_relevance: search_params.filters.min_relevance,
                time_range: self.parse_time_range(&search_params.filters.time_range),
                search_mode: match &search_params.filters.search_mode {
                    crate::agentic::SearchModeType::Semantic => shared::models::SearchMode::Vector,
                    crate::agentic::SearchModeType::Keyword => shared::models::SearchMode::FullText,
                    crate::agentic::SearchModeType::Hybrid => shared::models::SearchMode::Hybrid {
                        vector_weight: 0.5,
                        text_weight: 0.5,
                    },
                },
                boost_keywords: search_params.filters.boost_keywords.clone(),
            }),
        };

        self.shared_context.context_engine().execute(params).await
    }

    fn parse_time_range(
        &self,
        time_range_str: &Option<String>,
    ) -> Option<shared::models::TimeRange> {
        use chrono::{Duration, Utc};

        time_range_str.as_ref().and_then(|range_str| {
            let now = Utc::now();
            let start = match range_str.as_str() {
                "last_hour" => now - Duration::hours(1),
                "last_day" => now - Duration::days(1),
                "last_week" => now - Duration::weeks(1),
                "last_month" => now - Duration::days(30),
                "last_year" => now - Duration::days(365),
                _ => return None,
            };

            Some(shared::models::TimeRange { start, end: now })
        })
    }
}
