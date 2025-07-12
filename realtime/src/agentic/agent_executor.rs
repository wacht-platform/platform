use crate::agentic::{
    ContextEngineExecutor, ContextGatheringResponse, DecayManager, ExecutableTask, ExecutionAction,
    ExecutionStatus, IdeationResponse, LoopDecision, ParameterGenerationResponse,
    TaskBreakdownResponse, TaskExecutionResponse, TaskType, ToolExecutor, ValidationResponse,
    WorkflowExecutor, gemini_client::GeminiClient,
};
use crate::template::{AgentTemplates, render_template};
use llm::chat::{ChatMessage, ChatRole, MessageType};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared::commands::{Command, CreateConversationCommand};
use shared::dto::json::{StreamEvent, Task};
use shared::error::AppError;
use shared::models::{
    AiAgentWithFeatures, AiTool, AiToolConfiguration, ContextAction, ContextEngineParams,
    ContextFilters, ContextSearchResult, MemoryRecordV2,
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
    pub conversations: Vec<ChatMessage>,
    tool_executor: ToolExecutor,
    workflow_executor: WorkflowExecutor,
    decay_manager: DecayManager,
    context_engine_executor: ContextEngineExecutor,
    channel: tokio::sync::mpsc::Sender<StreamEvent>,
    memories: Vec<MemoryRecordV2>,
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

        // Log agent capabilities
        tracing::info!("🤖 Agent Executor initialized for agent: {} (ID: {})", agent.name, agent.id);
        
        // Log all tools
        tracing::info!("🔧 Agent tools ({} total):", agent.tools.len());
        for (idx, tool) in agent.tools.iter().enumerate() {
            tracing::info!("  {}. {} - {}", idx + 1, tool.name, tool.description.as_deref().unwrap_or("No description"));
            match &tool.configuration {
                shared::models::AiToolConfiguration::Api(config) => {
                    let method_str = match config.method {
                        shared::models::HttpMethod::GET => "GET",
                        shared::models::HttpMethod::POST => "POST",
                        shared::models::HttpMethod::PUT => "PUT",
                        shared::models::HttpMethod::DELETE => "DELETE",
                        shared::models::HttpMethod::PATCH => "PATCH",
                    };
                    tracing::info!("     Type: API | Endpoint: {} | Method: {}", config.endpoint, method_str);
                }
                shared::models::AiToolConfiguration::KnowledgeBase(config) => {
                    tracing::info!("     Type: Knowledge Base | KB ID: {}", config.knowledge_base_id);
                }
                shared::models::AiToolConfiguration::PlatformEvent(config) => {
                    tracing::info!("     Type: Platform Event | Event: {}", config.event_label);
                }
                shared::models::AiToolConfiguration::PlatformFunction(config) => {
                    tracing::info!("     Type: Platform Function | Function: {}", config.function_name);
                }
            }
        }
        
        // Log all workflows
        tracing::info!("⚡ Agent workflows ({} total):", agent.workflows.len());
        for (idx, workflow) in agent.workflows.iter().enumerate() {
            tracing::info!("  {}. {} - {}", 
                idx + 1, 
                workflow.name, 
                workflow.description.as_deref().unwrap_or("No description")
            );
            tracing::info!("     Nodes: {} | Version: {}", 
                workflow.workflow_definition.nodes.len(),
                workflow.workflow_definition.version
            );
        }
        
        // Log knowledge bases
        tracing::info!("📚 Agent knowledge bases ({} total):", agent.knowledge_bases.len());
        for (idx, kb) in agent.knowledge_bases.iter().enumerate() {
            tracing::info!("  {}. {} - {}", 
                idx + 1, 
                kb.name, 
                kb.description.as_deref().unwrap_or("No description")
            );
        }

        Ok(Self {
            agent,
            app_state,
            context_id,
            current_tasks: Arc::new(Mutex::new(Vec::new())),
            tool_executor,
            workflow_executor,
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
            Some("gemini-2.5-flash".to_string()),
        ))
    }

    pub fn create_weak_llm(&self) -> Result<GeminiClient, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        Ok(GeminiClient::new(
            api_key,
            Some("gemini-2.5-flash-lite-preview-06-17".to_string()),
        ))
    }

    async fn store_conversation(
        &mut self,
        role: ChatRole,
        content: &str,
        conversation_json: Value,
        message_type: &str,
    ) -> Result<(), AppError> {
        // Add to local conversations array
        self.conversations.push(ChatMessage {
            content: content.to_string(),
            role: role,
            message_type: MessageType::Text,
        });

        // Store in database
        CreateConversationCommand::new(
            self.app_state.sf.next_id()? as i64,
            self.context_id,
            conversation_json,
            message_type.to_string(),
        )
        .execute(&self.app_state)
        .await?;

        Ok(())
    }

    fn get_conversation_history_for_llm(&self) -> Vec<Value> {
        self.conversations
            .iter()
            .map(|msg| {
                let role = match msg.role {
                    ChatRole::User => "user",
                    ChatRole::Assistant => "model",
                };
                json!({
                    "role": role,
                    "content": msg.content.clone()
                })
            })
            .collect()
    }

    pub async fn execute_with_streaming(&mut self, user_message: &str) -> Result<(), AppError> {
        let user_message_json = json!({
            "role": "user",
            "content": user_message,
            "timestamp": chrono::Utc::now()
        });

        self.store_conversation(
            ChatRole::User,
            user_message,
            user_message_json,
            "user_message",
        )
        .await?;

        let immediate_context = self
            .decay_manager
            .get_immediate_context(self.context_id)
            .await?;

        self.memories = immediate_context.memories;
        self.conversations = immediate_context
            .conversations
            .iter()
            .map(|v| {
                let content = v
                    .content
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();

                if v.message_type == "user_message" {
                    ChatMessage::user().content(content).build()
                } else {
                    ChatMessage::assistant().content(content).build()
                }
            })
            .collect::<Vec<_>>();

        let acknowledgment_response = self.generate_acknowledgment().await?;

        if acknowledgment_response.further_action_required {
            self.execute_task_execution_loop().await?;
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

        let (raw, parsed) = self
            .create_weak_llm()?
            .generate_structured_content::<AcknowledgmentResponse>(request_body)
            .await?;

        let _ = self
            .channel
            .send(StreamEvent::Token(parsed.acknowledgment_message.clone()))
            .await;

        self.conversations
            .push(ChatMessage::assistant().content(raw.clone()).build());

        // Store acknowledgment as JSON
        let acknowledgment_json = json!({
            "role": "assistant",
            "content": parsed.acknowledgment_message.clone(),
            "raw_response": raw,
            "further_action_required": parsed.further_action_required,
            "reasoning": parsed.reasoning,
            "timestamp": chrono::Utc::now()
        });

        CreateConversationCommand::new(
            self.app_state.sf.next_id()? as i64,
            self.context_id,
            acknowledgment_json,
            "assistant_acknowledgment".to_string(),
        )
        .execute(&self.app_state)
        .await?;

        Ok(parsed)
    }

    async fn execute_task_execution_loop(&mut self) -> Result<(), AppError> {
        const MAX_LOOP_ITERATIONS: usize = 5;
        let mut loop_iteration = 0;
        let mut final_validation_response: Option<ValidationResponse> = None;
        let mut previous_errors: Vec<String> = Vec::new();
        let user_request = self
            .conversations
            .last()
            .and_then(|msg| {
                if msg.role == ChatRole::User {
                    Some(msg.content.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        while loop_iteration < MAX_LOOP_ITERATIONS {
            loop_iteration += 1;

            let ideation_response = self.execute_ideation_step(&user_request).await?;

            // Check if user input is required during ideation
            if ideation_response.requires_user_input {
                if let Some(user_request_message) = &ideation_response.user_input_request {
                    // Send user input request and wait for response
                    let _ = self
                        .channel
                        .send(StreamEvent::Token(format!("❓ **User Input Required:** {}", user_request_message)))
                        .await;
                    
                    // Store user input request in conversation
                    let user_input_json = json!({
                        "role": "assistant",
                        "content": user_request_message,
                        "message_type": "user_input_request",
                        "timestamp": chrono::Utc::now()
                    });

                    self.store_conversation(
                        ChatRole::Assistant,
                        user_request_message,
                        user_input_json,
                        "assistant_task_execution",
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
                        let _ = self
                            .channel
                            .send(StreamEvent::Token(format!("❓ **User Input Required:** {}", user_request_message)))
                            .await;
                        
                        // Store user input request in conversation
                        let user_input_json = json!({
                            "role": "assistant",
                            "content": user_request_message,
                            "message_type": "user_input_request",
                            "timestamp": chrono::Utc::now()
                        });

                        self.store_conversation(
                            ChatRole::Assistant,
                            user_request_message,
                            user_input_json,
                            "assistant_task_execution",
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
                            if let Some(user_request_message) = &current_context_response.user_input_request {
                                // Send user input request and wait for response
                                let _ = self
                                    .channel
                                    .send(StreamEvent::Token(format!("❓ **User Input Required:** {}", user_request_message)))
                                    .await;
                                
                                // Store user input request in conversation
                                let user_input_json = json!({
                                    "role": "assistant",
                                    "content": user_request_message,
                                    "message_type": "user_input_request",
                                    "timestamp": chrono::Utc::now()
                                });

                                self.store_conversation(
                                    ChatRole::Assistant,
                                    user_request_message,
                                    user_input_json,
                                    "assistant_task_execution",
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
                    let _ = self
                        .channel
                        .send(StreamEvent::Token("⚠️ **Detected repeated identical errors - checking if resolvable...**".to_string()))
                        .await;
                }
                previous_errors.push(error_signature);
            }

            let validation_response = self
                .execute_validation_step(
                    &user_request,
                    &ideation_response.execution_plan,
                    &execution_results,
                    loop_iteration,
                    MAX_LOOP_ITERATIONS,
                )
                .await?;

            self.channel
                .send(StreamEvent::Token(validation_response.user_message.clone()))
                .await
                .map_err(|_| AppError::Internal("Failed to send message".to_string()))?;

            match validation_response.loop_decision {
                LoopDecision::Complete => {
                    final_validation_response = Some(validation_response);
                    break;
                }
                LoopDecision::AbortUnresolvable => {
                    // Send unresolvable error message immediately
                    if let Some(error_details) = &validation_response.unresolvable_error_details {
                        let _ = self
                            .channel
                            .send(StreamEvent::Token(format!("\n\n❌ **Execution Aborted - Unresolvable Errors**\n\n{}", error_details)))
                            .await;
                    } else {
                        let _ = self
                            .channel
                            .send(StreamEvent::Token("\n\n❌ **Execution aborted due to unresolvable errors.**".to_string()))
                            .await;
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
                let _ = self
                    .channel
                    .send(StreamEvent::Token(format!("\n\n{}", final_summary)))
                    .await;
            }
            // Don't send any completion message if no final_summary
        } else {
            // Loop ended without completion decision (max iterations reached)
            let _ = self
                .channel
                .send(StreamEvent::Token("\n\n⚠️ **Task execution completed** (maximum iterations reached)".to_string()))
                .await;
        }

        Ok(())
    }

    async fn execute_ideation_step(
        &mut self,
        _user_request: &str,
    ) -> Result<IdeationResponse, AppError> {
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

            let (raw, parsed) = self
                .create_strong_llm()?
                .generate_structured_content::<IdeationResponse>(request_body)
                .await?;

            let ideation_json = json!({
                "role": "assistant",
                "content": raw.clone(),
                "ideation_response": parsed,
                "iteration": iteration,
                "timestamp": chrono::Utc::now()
            });

            self.store_conversation(
                ChatRole::Assistant,
                &raw,
                ideation_json,
                "assistant_ideation",
            )
            .await?;

            // Handle context search if requested
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

        let (raw, parsed) = self
            .create_weak_llm()?
            .generate_structured_content::<ContextGatheringResponse>(request_body)
            .await?;

        self.conversations
            .push(ChatMessage::assistant().content(&raw).build());

        // Store context gathering response as JSON
        let context_json = json!({
            "role": "assistant",
            "content": raw.clone(),
            "context_gathering_response": parsed,
            "timestamp": chrono::Utc::now()
        });

        CreateConversationCommand::new(
            self.app_state.sf.next_id()? as i64,
            self.context_id,
            context_json,
            "assistant_ideation".to_string(),
        )
        .execute(&self.app_state)
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

        let (raw, parsed) = self
            .create_strong_llm()?
            .generate_structured_content::<TaskBreakdownResponse>(request_body)
            .await?;

        let breakdown_json = json!({
            "role": "assistant",
            "content": raw.clone(),
            "task_breakdown_response": parsed,
            "timestamp": chrono::Utc::now()
        });

        self.store_conversation(
            ChatRole::Assistant,
            &raw,
            breakdown_json,
            "assistant_task_execution",
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

        let (raw, parsed) = self
            .create_weak_llm()?
            .generate_structured_content::<TaskExecutionResponse>(request_body)
            .await?;

        let task_exec_json = json!({
            "role": "assistant",
            "content": raw.clone(),
            "task_execution_response": parsed,
            "task_id": task.id,
            "task_name": task.name,
            "timestamp": chrono::Utc::now()
        });

        self.store_conversation(
            ChatRole::Assistant,
            &raw,
            task_exec_json,
            "assistant_task_execution",
        )
        .await?;

        let mut task_results = Vec::new();

        if matches!(parsed.execution_status, ExecutionStatus::Ready) {
            for action in &parsed.task_execution.actions.actions {
                match self.execute_action(action, task, &parsed).await {
                    Ok(action_result) => {
                        let action_result_json = json!({
                            "role": "assistant",
                            "content": serde_json::to_string(&action_result).unwrap(),
                            "action_result": action_result.clone(),
                            "task_id": task.id.clone(),
                            "timestamp": chrono::Utc::now()
                        });

                        self.store_conversation(
                            ChatRole::Assistant,
                            &serde_json::to_string(&action_result).unwrap(),
                            action_result_json,
                            "assistant_task_execution",
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
        user_request: &str,
        execution_plan: &crate::agentic::ExecutionPlan,
        execution_results: &[TaskExecutionResult],
        current_iteration: usize,
        max_iterations: usize,
    ) -> Result<ValidationResponse, AppError> {
        let validation_context = json!({
            "user_request": user_request,
            "execution_plan": execution_plan,
            "executed_tasks": execution_results.iter().map(|r| json!({
                "id": r.task_id,
                "name": r.task_name,
                "type": r.task_type,
                "status": r.status,
                "success_criteria": "Task specific criteria", // Would come from the actual task
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

        println!("{request_body} {:?}", self.conversations);

        let (raw, parsed) = self
            .create_weak_llm()?
            .generate_structured_content::<ValidationResponse>(request_body)
            .await?;

        // Store validation response as JSON
        let validation_json = json!({
            "role": "assistant",
            "content": raw.clone(),
            "validation_response": parsed,
            "loop_iteration": current_iteration,
            "timestamp": chrono::Utc::now()
        });

        self.store_conversation(
            ChatRole::Assistant,
            &raw,
            validation_json,
            "assistant_validation",
        )
        .await?;

        // Send user message if present
        if !parsed.user_message.is_empty() {
            let _ = self
                .channel
                .send(StreamEvent::Token(parsed.user_message.clone()))
                .await;
        }

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
            "trigger_condition": trigger_config.trigger_condition,
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
