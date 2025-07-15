use crate::agentic::{
    ContextEngineExecutor, ContextGatheringResponse, DecayManager, ExecutableTask, ExecutionAction,
    ExecutionStatus, IdeationResponse, LoopDecision, ParameterGenerationResponse,
    TaskBreakdownResponse, TaskExecutionResponse, TaskType, ToolExecutor, ValidationResponse,
    WorkflowExecutor, gemini_client::GeminiClient,
};
use crate::template::{AgentTemplates, render_template};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared::commands::{Command, CreateConversationCommand};
use shared::dto::json::{StreamEvent, Task};
use shared::error::AppError;
use shared::models::{
    AiAgentWithFeatures, AiTool, AiToolConfiguration, ContextAction, ContextEngineParams,
    ContextFilters, ContextSearchResult, ConversationContent, ConversationMessageType,
    ConversationRecord, MemoryRecordV2,
};
use shared::state::AppState;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename = "response")]
pub struct AcknowledgmentResponse {
    #[serde(rename = "message")]
    pub acknowledgment_message: String,
    pub further_action_required: bool,
    pub reasoning: String,
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
    pub current_tasks: Arc<Mutex<Vec<Task>>>,
    pub conversations: Vec<ConversationRecord>,
    tool_executor: ToolExecutor,
    workflow_executor: WorkflowExecutor,
    decay_manager: DecayManager,
    context_engine_executor: ContextEngineExecutor,
    channel: tokio::sync::mpsc::Sender<StreamEvent>,
    memories: Vec<MemoryRecordV2>,
    user_request: String,
}

impl AgentExecutor {
    pub async fn new(
        agent: AiAgentWithFeatures,
        context_id: i64,
        app_state: AppState,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Self, AppError> {
        let tool_executor = ToolExecutor::new(app_state.clone());
        let workflow_executor = WorkflowExecutor::new(app_state.clone());
        let decay_manager = DecayManager::new(app_state.clone());
        let context_engine_executor =
            ContextEngineExecutor::new(app_state.clone(), context_id, agent.id);

        Ok(Self {
            agent,
            app_state,
            context_id,
            current_tasks: Arc::new(Mutex::new(Vec::new())),
            tool_executor,
            workflow_executor,
            user_request: "".to_string(),
            decay_manager,
            context_engine_executor,
            channel,
            memories: Vec::new(),
            conversations: Vec::new(),
        })
    }

    pub fn create_strong_llm(&self) -> Result<GeminiClient, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        Ok(GeminiClient::new(
            api_key,
            Some("gemini-2.5-pro".to_string()),
        ))
    }

    pub fn create_weak_llm(&self) -> Result<GeminiClient, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

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
            timestamp: chrono::Utc::now(),
        }
    }

    async fn store_conversation(
        &mut self,
        typed_content: shared::models::ConversationContent,
        message_type: shared::models::ConversationMessageType,
    ) -> Result<(), AppError> {
        let message = CreateConversationCommand::new(
            self.app_state.sf.next_id()? as i64,
            self.context_id,
            typed_content.clone(),
            message_type.clone(),
        )
        .execute(&self.app_state)
        .await?;
        self.conversations.push(message.clone());

        let _ = self
            .channel
            .send(StreamEvent::ConversationMessage(message))
            .await;

        Ok(())
    }

    fn get_conversation_history_for_llm(&self) -> Vec<Value> {
        self.conversations
            .iter()
            .map(|msg| {
                let role = match msg.message_type {
                    ConversationMessageType::UserMessage => "user",
                    _ => "model",
                };
                json!({
                    "role": role,
                    "content": serde_json::to_string(&msg.content).unwrap()
                })
            })
            .collect()
    }

    pub async fn execute_with_streaming(&mut self, user_message: &str) -> Result<(), AppError> {
        let immediate_context = self
            .decay_manager
            .get_immediate_context(self.context_id)
            .await?;

        self.memories = immediate_context.memories;
        self.conversations = immediate_context.conversations;

        let user_content = ConversationContent::UserMessage {
            message: user_message.to_string(),
            timestamp: chrono::Utc::now(),
        };

        self.store_conversation(user_content, ConversationMessageType::UserMessage)
            .await?;

        let acknowledgment_response = self.generate_acknowledgment().await?;

        println!("{acknowledgment_response:?}");

        if acknowledgment_response.further_action_required {
            self.execute_task_execution_loop().await.unwrap();
        }

        Ok(())
    }

    async fn generate_acknowledgment(&mut self) -> Result<AcknowledgmentResponse, AppError> {
        let acknowledgment_context = json!({
            "tools": &self.agent.tools,
            "workflows": &self.agent.workflows,
            "knowledge_bases": &self.agent.knowledge_bases,
            "conversation_history": self.get_conversation_history_for_llm(),
        });

        let request_body = render_template(AgentTemplates::ACKNOWLEDGMENT, &acknowledgment_context)
            .map_err(|e| {
                AppError::Internal(format!("Failed to render acknowledgment template: {}", e))
            })?;

        let (_, parsed) = self
            .create_weak_llm()?
            .generate_structured_content::<AcknowledgmentResponse>(request_body)
            .await?;

        println!("{}", parsed.further_action_required);

        let acknowledgment_content = ConversationContent::AssistantAcknowledgment {
            acknowledgment_message: parsed.acknowledgment_message.clone(),
            further_action_required: parsed.further_action_required,
            reasoning: parsed.reasoning.clone(),
            timestamp: chrono::Utc::now(),
        };

        self.store_conversation(
            acknowledgment_content,
            ConversationMessageType::AssistantAcknowledgment,
        )
        .await?;

        Ok(parsed)
    }

    async fn execute_task_execution_loop(&mut self) -> Result<(), AppError> {
        const MAX_LOOP_ITERATIONS: usize = 5;
        let mut loop_iteration = 0;
        let mut final_validation_response: Option<ValidationResponse> = None;
        let mut previous_errors: Vec<String> = Vec::new();
        println!("here hh");

        while loop_iteration < MAX_LOOP_ITERATIONS {
            loop_iteration += 1;

            println!("here h1");

            let ideation_response = self.execute_ideation_step().await.unwrap();

            println!("here566768787");

            if ideation_response.requires_user_input {
                if let Some(user_request_message) = &ideation_response.user_input_request {
                    // Send user input request and wait for response
                    // TODO: Store user input request messages using store_conversation

                    // Store user input request in conversation
                    let ideation_content = ConversationContent::AssistantIdeation {
                        reasoning_summary: ideation_response.reasoning_summary.clone(),
                        needs_more_iteration: ideation_response.needs_more_iteration,
                        context_search_request: ideation_response.context_search_request.clone(),
                        requires_user_input: ideation_response.requires_user_input,
                        user_input_request: ideation_response.user_input_request.clone(),
                        execution_plan: serde_json::to_value(&ideation_response.execution_plan)
                            .unwrap_or_default(),
                        timestamp: chrono::Utc::now(),
                    };

                    self.store_conversation(
                        ideation_content,
                        ConversationMessageType::AssistantIdeation,
                    )
                    .await?;

                    // Return early - execution will continue when user responds
                    return Ok(());
                }
            }

            let mut context_findings = Vec::new();
            if ideation_response.needs_more_iteration
                && ideation_response.context_search_request.is_some()
            {
                let search_query = ideation_response.context_search_request.as_ref().unwrap();
                let context_response = self
                    .execute_context_gathering_step(search_query, &ideation_response.execution_plan)
                    .await?;

                // Check if user input is required during context gathering
                if context_response.requires_user_input {
                    if let Some(user_request_message) = &context_response.user_input_request {
                        // Send user input request and wait for response
                        // TODO: Store user input request messages using store_conversation

                        // Store user input request during task execution
                        // Since this is a request for user input during task execution, we'll use TaskExecution content
                        let task_content = ConversationContent::AssistantTaskExecution {
                            task_execution: json!({
                                "approach": "Requesting user input",
                                "actions": { "action": [] },
                                "expected_result": user_request_message
                            }),
                            execution_status: "blocked".to_string(),
                            blocking_reason: Some("Waiting for user input".to_string()),
                            timestamp: chrono::Utc::now(),
                        };

                        self.store_conversation(
                            task_content,
                            ConversationMessageType::AssistantTaskExecution,
                        )
                        .await?;

                        // Return early - execution will continue when user responds
                        return Ok(());
                    }
                }

                context_findings = context_response.context_insights.clone();

                let mut additional_iterations = 0;
                let mut current_context_response = context_response;
                while current_context_response.needs_more_context && additional_iterations < 2 {
                    additional_iterations += 1;
                    if let Some(additional_query) =
                        &current_context_response.strategic_context_request
                    {
                        current_context_response = self
                            .execute_context_gathering_step(
                                additional_query,
                                &ideation_response.execution_plan,
                            )
                            .await?;

                        // Check if user input is required during additional context gathering
                        if current_context_response.requires_user_input {
                            if let Some(user_request_message) =
                                &current_context_response.user_input_request
                            {
                                // Send user input request and wait for response
                                // TODO: Store user input request messages using store_conversation

                                // Store user input request during task execution
                                let task_content = ConversationContent::AssistantTaskExecution {
                                    task_execution: json!({
                                        "approach": "Requesting user input during workflow execution",
                                        "actions": { "action": [] },
                                        "expected_result": user_request_message
                                    }),
                                    execution_status: "blocked".to_string(),
                                    blocking_reason: Some("Waiting for user input".to_string()),
                                    timestamp: chrono::Utc::now(),
                                };

                                self.store_conversation(
                                    task_content,
                                    ConversationMessageType::AssistantTaskExecution,
                                )
                                .await?;

                                // Return early - execution will continue when user responds
                                return Ok(());
                            }
                        }

                        context_findings.extend(current_context_response.context_insights);
                    }
                }
            }

            let task_breakdown = self
                .execute_task_breakdown_step(&ideation_response.execution_plan, &context_findings)
                .await?;

            let execution_results = self.execute_tasks(&task_breakdown.tasks).await?;

            // Collect current errors
            let current_errors: Vec<String> = execution_results
                .iter()
                .filter_map(|result| result.error.clone())
                .collect();

            // Check for repeated identical errors (sign of unresolvable issues)
            if !current_errors.is_empty() {
                let error_signature = current_errors.join("|");
                if previous_errors.contains(&error_signature) {
                    // Same errors appearing again - likely unresolvable
                    // TODO: Consider storing error detection messages using store_conversation
                }
                previous_errors.push(error_signature);
            }

            let validation_response = self
                .execute_validation_step(
                    &ideation_response.execution_plan,
                    &execution_results,
                    loop_iteration,
                    MAX_LOOP_ITERATIONS,
                )
                .await?;

            // Send validation user message using store_conversation
            if !validation_response.user_message.is_empty() {
                self.store_conversation(
                    self.create_agent_response_content(validation_response.user_message.clone()),
                    ConversationMessageType::AgentResponse,
                )
                .await?;
            }

            match validation_response.loop_decision {
                LoopDecision::Complete => {
                    final_validation_response = Some(validation_response);
                    break;
                }
                LoopDecision::AbortUnresolvable => {
                    // Send unresolvable error message immediately
                    if let Some(error_details) = &validation_response.unresolvable_error_details {
                    } else {
                    }
                    return Ok(());
                }
                LoopDecision::Continue => (),
            }
        }

        // Send final summary without success message
        if let Some(final_response) = final_validation_response {
            if let Some(final_summary) = &final_response.final_summary {
                // Just send the summary without any success header
                let summary_message = format!("\n\n{}", final_summary);
                self.store_conversation(
                    self.create_agent_response_content(summary_message.clone()),
                    ConversationMessageType::AgentResponse,
                )
                .await?;
            }
            // Don't send any completion message if no final_summary
        } else {
            // Loop ended without completion decision (max iterations reached)
        }

        Ok(())
    }

    async fn execute_ideation_step(&mut self) -> Result<IdeationResponse, AppError> {
        const MAX_ITERATIONS: usize = 3;
        let mut iteration = 0;
        let mut context_search_results: Vec<String> = Vec::new();
        let mut current_plan: Option<crate::agentic::ExecutionPlan> = None;

        while iteration < MAX_ITERATIONS {
            iteration += 1;

            let ideation_context = json!({
                "available_tools": self.agent.tools,
                "workflows": self.agent.workflows,
                "knowledge_bases": self.agent.knowledge_bases,
                "memories": self.memories.iter().map(|m| m.content.clone()).collect::<Vec<_>>(),
                "context_search_results": context_search_results,
                "conversation_history": self.get_conversation_history_for_llm(),
                "iteration": iteration,
                "max_iterations": MAX_ITERATIONS,
                "is_final_iteration": iteration == MAX_ITERATIONS,
                "current_plan": current_plan,
            });

            let request_body = render_template(AgentTemplates::IDEATION, &ideation_context)
                .map_err(|e| {
                    AppError::Internal(format!("Failed to render ideation template: {}", e))
                })?;

            let (_, parsed) = self
                .create_strong_llm()?
                .generate_structured_content::<IdeationResponse>(request_body)
                .await
                .unwrap();

            let ideation_content = ConversationContent::AssistantIdeation {
                reasoning_summary: parsed.reasoning_summary.clone(),
                needs_more_iteration: parsed.needs_more_iteration,
                context_search_request: parsed.context_search_request.clone(),
                requires_user_input: parsed.requires_user_input,
                user_input_request: parsed.user_input_request.clone(),
                execution_plan: serde_json::to_value(&parsed.execution_plan).unwrap_or_default(),
                timestamp: chrono::Utc::now(),
            };

            self.store_conversation(ideation_content, ConversationMessageType::AssistantIdeation)
                .await
                .unwrap();

            if parsed.needs_more_iteration && parsed.context_search_request.is_some() {
                let search_query = parsed.context_search_request.as_ref().unwrap();
                let search_results = self.search_context(search_query).await?;
                context_search_results.extend(search_results.iter().map(|r| r.content.clone()));
                current_plan = Some(parsed.execution_plan.clone());
            } else {
                return Ok(parsed);
            }

            if iteration >= MAX_ITERATIONS {
                return Ok(parsed);
            }
        }

        Err(AppError::Internal(
            "Failed to complete ideation after max iterations".to_string(),
        ))
    }

    async fn execute_context_gathering_step(
        &mut self,
        search_query: &str,
        execution_plan: &crate::agentic::ExecutionPlan,
    ) -> Result<ContextGatheringResponse, AppError> {
        let context_results = self.search_context(search_query).await?;

        let context_gathering_context = json!({
            "current_plan": execution_plan,
            "context_search_query": search_query,
            "context_results": context_results.iter().map(|r| json!({
                "source_type": match &r.source {
                    shared::models::ContextSource::KnowledgeBase { .. } => "knowledge_base",
                    shared::models::ContextSource::Memory { category, .. } => category,
                    shared::models::ContextSource::DynamicContext { .. } => "dynamic_context",
                    shared::models::ContextSource::Conversation { .. } => "conversation",
                },
                "source_details": match &r.source {
                    shared::models::ContextSource::KnowledgeBase { kb_id, .. } => format!("KB {}", kb_id),
                    shared::models::ContextSource::Memory { memory_id, category } => format!("{} memory {}", category, memory_id),
                    shared::models::ContextSource::DynamicContext { context_type } => context_type.clone(),
                    shared::models::ContextSource::Conversation { conversation_id } => format!("Conversation {}", conversation_id),
                },
                "relevance_score": r.relevance_score,
                "content": r.content,
                "metadata": r.metadata,
            })).collect::<Vec<_>>(),
            "available_tools": self.agent.tools.len(),
            "workflows": self.agent.workflows.len(),
            "knowledge_bases": self.agent.knowledge_bases.len(),
            "memories": self.memories.len(),
            "conversation_history": self.get_conversation_history_for_llm(),
        });

        let request_body = render_template(
            AgentTemplates::CONTEXT_GATHERING,
            &context_gathering_context,
        )
        .map_err(|e| {
            AppError::Internal(format!(
                "Failed to render context gathering template: {}",
                e
            ))
        })?;

        let (_, parsed) = self
            .create_weak_llm()?
            .generate_structured_content::<ContextGatheringResponse>(request_body)
            .await?;

        let ideation_content = ConversationContent::AssistantIdeation {
            reasoning_summary: format!("Context gathering for: {}", search_query),
            needs_more_iteration: false,
            context_search_request: Some(search_query.to_string()),
            requires_user_input: false,
            user_input_request: None,
            execution_plan: serde_json::to_value(execution_plan).unwrap_or_default(),
            timestamp: chrono::Utc::now(),
        };

        self.store_conversation(ideation_content, ConversationMessageType::AssistantIdeation)
            .await?;

        Ok(parsed)
    }

    async fn execute_task_breakdown_step(
        &mut self,
        execution_plan: &crate::agentic::ExecutionPlan,
        context_findings: &[String],
    ) -> Result<TaskBreakdownResponse, AppError> {
        let task_breakdown_context = json!({
            "execution_plan": execution_plan,
            "context_findings": context_findings,
            "available_tools": self.agent.tools,
            "workflows": self.agent.workflows,
            "knowledge_bases": self.agent.knowledge_bases,
            "conversation_history": self.get_conversation_history_for_llm(),
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
            timestamp: chrono::Utc::now(),
        };

        self.store_conversation(
            task_content,
            ConversationMessageType::AssistantTaskExecution,
        )
        .await?;

        Ok(parsed)
    }

    async fn execute_tasks(
        &mut self,
        tasks: &[ExecutableTask],
    ) -> Result<Vec<TaskExecutionResult>, AppError> {
        let mut results = Vec::new();
        let mut completed_tasks = std::collections::HashMap::new();

        for task in tasks {
            let dependencies_met = task.dependencies.iter().all(|dep_id| {
                completed_tasks
                    .get(dep_id)
                    .map_or(false, |r: &TaskExecutionResult| r.success)
            });

            if !dependencies_met {
                results.push(TaskExecutionResult {
                    task_id: task.id.clone(),
                    task_name: task.name.clone(),
                    task_type: "task".to_string(),
                    status: "skipped".to_string(),
                    success: false,
                    result: None,
                    error: Some("Dependencies not met".to_string()),
                });
                continue;
            }

            let task_result = self.execute_single_task(task, &completed_tasks).await?;

            completed_tasks.insert(task.id.clone(), task_result.clone());
            results.push(task_result);
        }

        Ok(results)
    }

    async fn execute_single_task(
        &mut self,
        task: &ExecutableTask,
        previous_results: &std::collections::HashMap<String, TaskExecutionResult>,
    ) -> Result<TaskExecutionResult, AppError> {
        let task_execution_context = json!({
            "current_task": task,
            "dependencies": task.dependencies.iter().map(|dep_id| {
                let status = previous_results.get(dep_id)
                    .map(|r| if r.success { "completed" } else { "failed" })
                    .unwrap_or("not_found");
                json!({
                    "task_id": dep_id,
                    "status": status,
                    "result": previous_results.get(dep_id).and_then(|r| r.result.clone()),
                })
            }).collect::<Vec<_>>(),
            "previous_results": previous_results.iter().map(|(id, result)| json!({
                "task_id": id,
                "summary": result.result.as_ref().map(|r| r.to_string()).unwrap_or_default(),
            })).collect::<Vec<_>>(),
            "available_tools": self.agent.tools,
            "workflows": self.agent.workflows,
            "conversation_history": self.get_conversation_history_for_llm(),
        });

        let request_body = render_template(AgentTemplates::TASK_EXECUTION, &task_execution_context)
            .map_err(|e| {
                AppError::Internal(format!("Failed to render task execution template: {}", e))
            })?;

        let (_, parsed) = self
            .create_weak_llm()?
            .generate_structured_content::<TaskExecutionResponse>(request_body)
            .await?;

        // Store task execution response with typed content
        let task_content = ConversationContent::AssistantTaskExecution {
            task_execution: serde_json::to_value(&parsed.task_execution).unwrap_or_default(),
            execution_status: match &parsed.execution_status {
                ExecutionStatus::Ready => "ready",
                ExecutionStatus::Blocked => "blocked",
                ExecutionStatus::CannotExecute => "cannot_execute",
            }
            .to_string(),
            blocking_reason: parsed.blocking_reason.clone(),
            timestamp: chrono::Utc::now(),
        };

        self.store_conversation(
            task_content,
            ConversationMessageType::AssistantTaskExecution,
        )
        .await?;

        let mut task_results = Vec::new();

        if matches!(parsed.execution_status, ExecutionStatus::Ready) {
            for action in &parsed.task_execution.actions.actions {
                match self.execute_action(action, task, &parsed).await {
                    Ok(action_result) => {
                        let action_result_content = ConversationContent::AssistantTaskExecution {
                            task_execution: json!({
                                "approach": format!("Executed action: {}", action.purpose),
                                "actions": { "action": [action] },
                                "expected_result": "Action completed"
                            }),
                            execution_status: "ready".to_string(),
                            blocking_reason: None,
                            timestamp: chrono::Utc::now(),
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

    async fn execute_validation_step(
        &mut self,
        execution_plan: &crate::agentic::ExecutionPlan,
        execution_results: &[TaskExecutionResult],
        current_iteration: usize,
        max_iterations: usize,
    ) -> Result<ValidationResponse, AppError> {
        let validation_context = json!({
            "user_request": self.user_request.clone(),
            "execution_plan": execution_plan,
            "executed_tasks": execution_results.iter().map(|r| json!({
                "id": r.task_id,
                "name": r.task_name,
                "type": r.task_type,
                "status": r.status,
                "success_criteria": "Task specific criteria",
                "result": r.result,
                "error": r.error,
            })).collect::<Vec<_>>(),
            "current_iteration": current_iteration,
            "max_iterations": max_iterations,
            "conversation_history": self.get_conversation_history_for_llm(),
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
            final_summary: parsed.final_summary.clone(),
            has_unresolvable_errors: parsed.has_unresolvable_errors,
            unresolvable_error_details: parsed.unresolvable_error_details.clone(),
            user_message: parsed.user_message.clone(),
            timestamp: chrono::Utc::now(),
        };

        self.store_conversation(
            validation_content,
            ConversationMessageType::AssistantValidation,
        )
        .await?;

        Ok(parsed)
    }

    async fn search_context(&self, query: &str) -> Result<Vec<ContextSearchResult>, AppError> {
        let params = ContextEngineParams {
            query: query.to_string(),
            action: ContextAction::SearchAll,
            filters: Some(ContextFilters {
                max_results: 10,
                min_relevance: 0.7,
                time_range: None,
            }),
        };

        self.context_engine_executor.execute(params).await
    }

    async fn execute_action(
        &mut self,
        action: &ExecutionAction,
        task: &ExecutableTask,
        _execution_response: &TaskExecutionResponse,
    ) -> Result<Value, AppError> {
        match &action.action_type {
            TaskType::ToolCall => {
                let tool_name = action
                    .details
                    .get("resource_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AppError::Internal("Tool name not found in action details".to_string())
                    })?;

                let tool = self
                    .agent
                    .tools
                    .iter()
                    .find(|t| t.name == tool_name)
                    .ok_or_else(|| AppError::NotFound(format!("Tool {} not found", tool_name)))?;

                let parameters = self
                    .generate_parameters_for_tool(tool, action, task)
                    .await?;

                self.tool_executor
                    .execute_tool_immediately(tool, parameters)
                    .await
            }
            TaskType::WorkflowCall => {
                let workflow_name = action
                    .details
                    .get("resource_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AppError::Internal("Workflow name not found in action details".to_string())
                    })?;

                // Verify the workflow exists and clone it
                let workflow = self
                    .agent
                    .workflows
                    .iter()
                    .find(|w| w.name == workflow_name)
                    .ok_or_else(|| {
                        AppError::NotFound(format!("Workflow {} not found", workflow_name))
                    })?
                    .clone();

                // Prepare workflow call inputs from action details
                let inputs = action.details.get("inputs").cloned().unwrap_or(json!({}));

                let workflow_call = shared::dto::json::WorkflowCall {
                    workflow_name: workflow_name.to_string(),
                    inputs,
                };

                // Execute workflow with context gathering loop (following acknowledgment flow pattern)
                self.execute_workflow_with_context_gathering(workflow_call, &workflow)
                    .await
            }
            TaskType::KnowledgeSearch => {
                let query = action
                    .details
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AppError::Internal("Query not found in action details".to_string())
                    })?;

                let results = self.search_context(query).await?;
                Ok(json!({
                    "search_type": "knowledge",
                    "query": query,
                    "results": results,
                    "result_count": results.len()
                }))
            }
            TaskType::ContextSearch => {
                let query = action
                    .details
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AppError::Internal("Query not found in action details".to_string())
                    })?;

                let results = self.search_context(query).await?;
                Ok(json!({
                    "search_type": "context",
                    "query": query,
                    "results": results,
                    "result_count": results.len()
                }))
            }
        }
    }

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
            "previous_results": self.current_tasks.lock().await.iter()
                .map(|t| json!({
                    "task_id": t.id,
                    "summary": t.status
                }))
                .collect::<Vec<_>>(),
            "context_findings": self.memories.iter()
                .map(|m| m.content.clone())
                .take(5)
                .collect::<Vec<_>>(),
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
}
