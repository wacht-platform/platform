use super::core::{AgentExecutor, ResumeContext};
use crate::gemini::GeminiClient;
use crate::template::{render_template_with_prompt, AgentTemplates};

use commands::{Command, UpdateExecutionContextQuery};
use queries::Query;
use common::error::AppError;
use dto::json::agent_executor::{
    ContextGatheringDirective, ConverseRequest, DeepReasoningDirective, NextStep, ObjectiveDefinition, StepDecision,
};
use dto::json::agent_responses::{
    ActionsList, TaskExecution, TaskType,
};
use dto::json::{
    StepDecisionContext, StreamEvent, UserInputOutputState,
    WorkflowExecutionResult, WorkflowTaskExecution,
};
use models::{
    AgentExecutionState, ConversationContent, ConversationMessageType, ExecutionContextStatus,
};
use serde_json::{json, Value};


const MAX_LOOP_ITERATIONS: usize = 50;

impl AgentExecutor {
    pub async fn resume_execution(
        &mut self,
        resume_context: ResumeContext,
    ) -> Result<(), AppError> {
        let context_id = self.context_id;
        let deployment_id = self.agent.deployment_id;
        let app_state = self.app_state.clone();

        let immediate_context = self.get_immediate_context().await?;
        self.conversations = immediate_context.conversations;
        self.memories = immediate_context.memories;

        match resume_context {
            ResumeContext::PlatformFunction(execution_id, result) => {
                let conversation = self
                    .create_conversation(
                        ConversationContent::PlatformFunctionResult {
                            execution_id: execution_id.clone(),
                            result: serde_json::to_string(&result)
                                .unwrap_or_else(|_| result.to_string()),
                        },
                        ConversationMessageType::PlatformFunctionResult,
                    )
                    .await?;

                self.conversations.push(conversation.clone());
                let _ = self
                    .channel
                    .send(StreamEvent::ConversationMessage(conversation))
                    .await;

                if let Some(workflow_state) = &mut self.current_workflow_state {
                    for (key, value) in workflow_state.clone().iter() {
                        if key.ends_with("_output") {
                            if let Some(stored_exec_id) =
                                value.get("execution_id").and_then(|v| v.as_str())
                            {
                                if stored_exec_id == &execution_id {
                                    workflow_state.insert(key.clone(), result.clone());
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            ResumeContext::UserInput(input) => {
                let conversation = self.store_user_message(input.clone(), None).await?;
                if let Some(workflow_state) = &mut self.current_workflow_state {
                    if let Some(node_id) = &self.current_workflow_node_id {
                        let node_output_key = format!("{}_output", node_id);
                        let user_input_output = UserInputOutputState {
                            value: input,
                            output_type: "user_input".to_string(),
                        };
                        workflow_state
                            .insert(node_output_key, serde_json::to_value(&user_input_output)?);
                    }
                } else {
                    self.conversations.push(conversation.clone());
                    let _ = self
                        .channel
                        .send(StreamEvent::ConversationMessage(conversation))
                        .await;
                }
            }
        }

        UpdateExecutionContextQuery::new(context_id, deployment_id)
            .with_status(ExecutionContextStatus::Running)
            .execute(&app_state)
            .await?;

        let result = self.repl().await;

        if let Err(e) = self.filesystem.cleanup().await {
            tracing::error!("Failed to cleanup filesystem: {}", e);
        }

        result
    }

    /// Execute with a pre-persisted conversation ID
    /// The conversation must already exist in the database
    pub async fn execute_with_conversation_id(
        &mut self,
        conversation_id: i64,
    ) -> Result<(), AppError> {
        let request = ConverseRequest { conversation_id };
        self.run(request).await
    }

    pub async fn run(&mut self, request: ConverseRequest) -> Result<(), AppError> {
        // Fetch the conversation from DB - it must already be persisted
        let conversation = queries::GetConversationByIdQuery::new(request.conversation_id)
            .execute(&self.app_state)
            .await?;

        // Extract user message from conversation content
        let (user_message, sender_name) = match &conversation.content {
            models::ConversationContent::UserMessage { message, sender_name, .. } => (message.clone(), sender_name.clone()),
            _ => return Err(AppError::BadRequest("Conversation must be a user message".to_string())),
        };

        // Log incoming message if from Teams integration
        if let Some(user) = &sender_name {
            // Fetch context to check source and get metadata
            if let Ok(ctx) = queries::GetExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
                .execute(&self.app_state)
                .await 
            {
                if ctx.source.as_deref() == Some("teams") {
                    if let Some(group) = &ctx.context_group {
                        if !group.is_empty() {
                            let mut location = String::new();
                            if let Some(meta) = &ctx.external_resource_metadata {
                                if let Some(channel_name) = meta.get("channelName").and_then(|v| v.as_str()) {
                                    location = format!(" [Channel: {}]", channel_name);
                                }
                            }
                            
                            let logger = crate::teams_logger::TeamsActivityLogger::new(
                                &self.agent.deployment_id.to_string(), 
                                group
                            );
                            let _ = logger.append_entry("INCOMING", &format!("From {}{}: {}", user, location, user_message)).await;
                        }
                    }
                }
            }
        }

        self.user_request = user_message;

        // Stream the conversation to subscribers (for realtime clients)
        let _ = self
            .channel
            .send(StreamEvent::ConversationMessage(conversation))
            .await;

        // get_immediate_context() fetches all conversations from DB including the one we just fetched
        let context = self.get_immediate_context().await?;

        self.conversations = context.conversations;
        self.memories = context.memories;

        let result = self.repl().await;

        if let Err(e) = self.filesystem.cleanup().await {
            tracing::error!("Failed to cleanup filesystem: {}", e);
        }

        result?;

        Ok(())
    }

    pub(super) async fn repl(&mut self) -> Result<(), AppError> {
        if let Some(workflow_id) = self.current_workflow_id {
            let result = self.resume_workflow_execution().await?;

            let workflow_result: WorkflowExecutionResult = serde_json::from_value(result)?;

            if workflow_result.execution_status == "pending" {
                let task_execution = WorkflowTaskExecution {
                    execution_type: "workflow".to_string(),
                    workflow_id,
                    result: workflow_result.clone(),
                };

                self.store_conversation(
                    ConversationContent::ActionExecutionResult {
                        task_execution: serde_json::to_value(&task_execution)?,
                        execution_status: "pending".to_string(),
                        blocking_reason: None,
                    },
                    ConversationMessageType::ActionExecutionResult,
                )
                .await?;

                return Ok(());
            }

            let task_execution = WorkflowTaskExecution {
                execution_type: "workflow".to_string(),
                workflow_id,
                result: workflow_result.clone(),
            };

            self.store_conversation(
                ConversationContent::ActionExecutionResult {
                    task_execution: serde_json::to_value(&task_execution)?,
                    execution_status: workflow_result.execution_status,
                    blocking_reason: None,
                },
                ConversationMessageType::ActionExecutionResult,
            )
            .await?;

            self.current_workflow_id = None;
            self.current_workflow_state = None;
            self.current_workflow_node_id = None;
            self.current_workflow_execution_path = Vec::new();
        }

        let mut iteration = 0;
        loop {
            iteration += 1;

            if iteration > MAX_LOOP_ITERATIONS {
                self.deliver_final_response().await?;
                return Ok(());
            }

            println!("Iteration: {}", iteration);

            let decision = self.decide_next_step().await?;

            println!("Decision: {:#?}", decision);

            match self.process_decision(decision).await {
                Ok(should_continue) => {
                    if !should_continue {
                        return Ok(());
                    }
                }
                Err(e) => {
                    self.store_conversation(
                        ConversationContent::SystemDecision {
                            step: "error_encountered".to_string(),
                            reasoning: format!("Encountered unexpected error: {}. Continuing with available information.", e),
                            confidence: 0.5,
                            thought_signature: None,
                        },
                        ConversationMessageType::SystemDecision,
                    ).await?;
                }
            }
        }
    }

    async fn process_decision(&mut self, decision: StepDecision) -> Result<bool, AppError> {

        let result = match decision.next_step {
            NextStep::Acknowledge => {
                if let Some(ack_data) = decision.acknowledgment {
                    self.store_conversation(
                        ConversationContent::AssistantAcknowledgment {
                            acknowledgment_message: ack_data.message,
                            further_action_required: ack_data.further_action_required,
                            reasoning: decision.reasoning.clone(),
                            thought_signature: decision.thought_signature.clone(),
                        },
                        ConversationMessageType::AssistantAcknowledgment,
                    )
                    .await?;

                    self.current_objective = Some(ack_data.objective);

                    Ok(ack_data.further_action_required)
                } else {
                    Err(AppError::Internal(
                        "Acknowledgment data missing for acknowledge step".to_string(),
                    ))
                }
            }

            NextStep::GatherContext => {
                let directive = decision.context_gathering_directive.ok_or_else(|| {
                    AppError::Internal(
                        "Context gathering directive is required for gathercontext step"
                            .to_string(),
                    )
                })?;

                match self.gather_context(directive).await {
                    Ok(_) => Ok(true),
                    Err(e) => Err(e),
                }
            }

            NextStep::LoadMemory => {
                let directive = decision.memory_loading_directive.ok_or_else(|| {
                    AppError::Internal(
                        "Memory loading directive is required for loadmemory step".to_string(),
                    )
                })?;

                self.load_memories_with_directive(directive).await?;
                Ok(true)
            }

            NextStep::ExecuteAction => {
                if let Some(actions) = decision.actions {
                    let actions_to_execute: Vec<_> = actions.into_iter().take(10).collect();
                    
                    let futures: Vec<_> = actions_to_execute
                        .iter()
                        .map(|action| self.execute_action(action))
                        .collect();
                    
                    let results = futures::future::join_all(futures).await;
                    
                    let mut all_results = Vec::new();
                    let mut any_pending = false;
                    
                    for (action, result) in actions_to_execute.iter().zip(results.into_iter()) {
                        match result {
                            Ok(result_value) => {
                                if result_value.get("status").and_then(|s| s.as_str()) == Some("pending") {
                                    any_pending = true;
                                } else {
                                    let task_type_str = match action.action_type {
                                        TaskType::ToolCall => "tool_call",
                                        TaskType::WorkflowCall => "workflow_call",
                                    };
                                    let task_id = format!(
                                        "{}_{}_{}",
                                        task_type_str,
                                        chrono::Utc::now().timestamp_millis(),
                                        all_results.len()
                                    );

                                    let task_result = dto::json::agent_executor::TaskExecutionResult {
                                        task_id: task_id.clone(),
                                        status: "completed".to_string(),
                                        output: Some(result_value.clone()),
                                        error: None,
                                    };

                                    self.task_results.insert(task_id, task_result);
                                }
                                all_results.push(serde_json::json!({
                                    "action": action.purpose,
                                    "status": "success",
                                    "result": result_value
                                }));
                            }
                            Err(e) => {
                                all_results.push(serde_json::json!({
                                    "action": action.purpose,
                                    "status": "error",
                                    "error": e.to_string()
                                }));
                            }
                        }
                    }

                    if any_pending {
                        let execution_state = AgentExecutionState {
                            task_results: self
                                .task_results
                                .iter()
                                .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap()))
                                .collect(),
                            current_objective: self
                                .current_objective
                                .as_ref()
                                .map(|o| serde_json::to_value(o).unwrap()),
                            conversation_insights: self
                                .conversation_insights
                                .as_ref()
                                .map(|c| serde_json::to_value(c).unwrap()),
                            workflow_state: None,
                            pending_input_request: None,
                        };

                        UpdateExecutionContextQuery::new(
                            self.context_id,
                            self.agent.deployment_id,
                        )
                        .with_execution_state(execution_state)
                        .with_status(ExecutionContextStatus::WaitingForInput)
                        .execute(&self.app_state)
                        .await?;
                    }

                    let execution = TaskExecution {
                        approach: format!("Executing {} action(s)", actions_to_execute.len()),
                        actions: ActionsList {
                            actions: actions_to_execute,
                        },
                        expected_result: "Execution results".to_string(),
                        actual_result: Some(serde_json::Value::Array(all_results)),
                    };

                    self.store_conversation(
                        ConversationContent::ActionExecutionResult {
                            task_execution: serde_json::to_value(&execution)?,
                            execution_status: if any_pending { "pending" } else { "completed" }.to_string(),
                            blocking_reason: None,
                        },
                        ConversationMessageType::ActionExecutionResult,
                    )
                    .await?;

                    if any_pending {
                        return Ok(false);
                    }
                }
                Ok(true)
            }

            NextStep::LongThinkAndReason => {
                if let Some(directive) = decision.deep_reasoning_directive {
                    let (reasoning_result, signature) = self.execute_deep_reasoning(&directive).await?;
                    
                    self.store_conversation(
                        ConversationContent::SystemDecision {
                            step: "deep_reasoning".to_string(),
                            reasoning: reasoning_result.analysis.clone(),
                            confidence: reasoning_result.confidence as f32,
                            thought_signature: signature,
                        },
                        ConversationMessageType::SystemDecision,
                    )
                    .await?;

                    Ok(true)
                } else {
                    Err(AppError::BadRequest(
                        "LongThinkAndReason requires deep_reasoning_directive".to_string(),
                    ))
                }
            }

            NextStep::RequestUserInput => {
                self.request_user_input().await?;
                Ok(false)
            }

            NextStep::Complete => {
                self.reinforce_used_memories().await?;

                if let Some(message) = &decision.completion_message {
                    self.store_conversation(
                        ConversationContent::AgentResponse {
                            response: message.clone(),
                            context_used: Default::default(),
                            thought_signature: decision.thought_signature.clone(),
                        },
                        ConversationMessageType::AgentResponse,
                    )
                    .await?;
                }

                UpdateExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
                    .with_status(ExecutionContextStatus::Idle)
                    .execute(&self.app_state)
                    .await?;
                Ok(false)
            }
        };

        result
    }

    async fn decide_next_step(&mut self) -> Result<StepDecision, AppError> {
        // Fetch the execution context to get title and actionables from metadata
        let exec_context = queries::GetExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
            .execute(&self.app_state)
            .await?;
        
        // Parse actionables from external_resource_metadata if present
        let actionables: Vec<dto::json::Actionable> = exec_context.external_resource_metadata
            .as_ref()
            .and_then(|m| m.get("actionables"))
            .and_then(|a| serde_json::from_value(a.clone()).ok())
            .unwrap_or_default();

        let context = StepDecisionContext {
            conversation_history: self.get_conversation_history_for_llm().await,
            user_request: self.user_request.clone(),
            current_objective: self
                .current_objective
                .as_ref()
                .map(|o| serde_json::to_value(o).unwrap()),
            conversation_insights: self
                .conversation_insights
                .as_ref()
                .map(|c| serde_json::to_value(c).unwrap()),
            task_results: self
                .task_results
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap()))
                .collect(),
            available_tools: self
                .agent
                .tools
                .iter()
                .map(|t| serde_json::to_value(t).unwrap())
                .collect(),
            available_workflows: self
                .agent
                .workflows
                .iter()
                .map(|w| serde_json::to_value(w).unwrap())
                .collect(),
            available_knowledge_bases: self
                .agent
                .knowledge_bases
                .iter()
                .map(|kb| serde_json::to_value(kb).unwrap())
                .collect(),
            iteration_info: dto::json::IterationInfo {
                current_iteration: 1,
                max_iterations: MAX_LOOP_ITERATIONS,
            },
            teams_enabled: self.teams_enabled,
            context_id: self.context_id,
            context_title: exec_context.title,
            actionables,
        };

        let mut context_json = serde_json::to_value(&context)?;
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

        let request_body = render_template_with_prompt(AgentTemplates::STEP_DECISION, context_json)
            .map_err(|e| {
                AppError::Internal(format!("Failed to render step decision template: {e}"))
            })?;


        let (mut decision, signature) = self
            .create_strong_llm()?
            .generate_structured_content::<StepDecision>(request_body)
            .await?;


        decision.thought_signature = signature.clone();

        if decision.acknowledgment.is_none() {
            self.store_conversation(
                ConversationContent::SystemDecision {
                    step: format!("{:?}", decision.next_step).to_lowercase(),
                    reasoning: decision.reasoning.clone(),
                    confidence: decision.confidence as f32,
                    thought_signature: signature,
                },
                ConversationMessageType::SystemDecision,
            )
            .await?;
        }

        Ok(decision)
    }

    async fn generate_and_send_summary(&mut self) -> Result<(), AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::SUMMARY,
            json!({
                "conversation_history": self.get_conversation_history_for_llm().await,
                "user_request": self.user_request,
                "task_results": self.task_results,
                "available_tools": self.agent.tools.clone(),
                "available_workflows": self.agent.workflows.clone(),
                "available_knowledge_bases": self.agent.knowledge_bases.clone(),
            }),
        )
        .map_err(|e| AppError::Internal(format!("Failed to render summary template: {e}")))?;

        let (summary, _) = self
            .create_weak_llm()?
            .generate_structured_content::<Value>(request_body)
            .await?;

        self.store_conversation(
            ConversationContent::AgentResponse {
                response: summary.get("response").unwrap().as_str().unwrap().into(),
                context_used: Default::default(),
                thought_signature: None,
            },
            ConversationMessageType::AgentResponse,
        )
        .await?;

        UpdateExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
            .with_status(ExecutionContextStatus::Idle)
            .execute(&self.app_state)
            .await?;

        Ok(())
    }

    async fn deliver_final_response(&mut self) -> Result<(), AppError> {
        self.generate_and_send_summary().await
    }

    async fn gather_context(
        &mut self,
        directive: ContextGatheringDirective,
    ) -> Result<(), AppError> {
        let context_objective = Some(ObjectiveDefinition {
            primary_goal: directive.objective.clone(),
            success_criteria: directive
                .focus_areas
                .clone()
                .unwrap_or_else(|| vec!["Find relevant information".to_string()]),
            constraints: vec![format!("Search pattern: {:?}", directive.pattern)],
            context_from_history: format!("Pattern-based search: {:?}", directive.pattern),
            inferred_intent: directive.objective.clone(),
        });

        let query_description = format!("[{:?}] {}", directive.pattern, directive.objective);

        let hints = match self
            .context_orchestrator
            .gather_context_hints(
                &self.conversations,
                &self.memories,
                &context_objective,
                directive.pattern,
                directive.expected_depth,
            )
            .await
        {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(
                    "Context gathering encountered an issue: {}. Continuing with empty hints.",
                    e
                );
                dto::json::agent_executor::ContextHints {
                    recommended_files: vec![],
                    search_summary: format!("Search failed: {}", e),
                    search_conclusion: dto::json::agent_executor::SearchConclusion::NothingFound,
                    search_terms_used: vec![],
                    knowledge_bases_searched: vec![],
                }
            }
        };

        self.store_conversation(
            ConversationContent::ContextResults {
                query: query_description,
                results: serde_json::to_value(&hints)?,
                result_count: hints.recommended_files.len(),
                timestamp: chrono::Utc::now(),
            },
            ConversationMessageType::ContextResults,
        )
        .await?;

        Ok(())
    }

    async fn request_user_input(&mut self) -> Result<(), AppError> {
        let input_request = self.generate_user_input_request().await?;
        let content = self.parse_user_input_request(&input_request)?;

        self.save_execution_state_for_input(&input_request).await?;

        self.store_conversation(content, ConversationMessageType::UserInputRequest)
            .await?;
        Ok(())
    }

    async fn generate_user_input_request(&self) -> Result<Value, AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::USER_INPUT_REQUEST,
            json!({
                "conversation_history": self.get_conversation_history_for_llm().await,
                "current_objective": self.current_objective,
                "working_memory": self.get_working_memory(),
                "available_tools": self.agent.tools.clone(),
                "available_workflows": self.agent.workflows.clone(),
                "available_knowledge_bases": self.agent.knowledge_bases.clone(),
            }),
        )
        .map_err(|e| {
            AppError::Internal(format!("Failed to render user input request template: {e}"))
        })?;

        let (response, _) = self.create_weak_llm()?
            .generate_structured_content::<serde_json::Value>(request_body)
            .await?;
        Ok(response)
    }

    fn parse_user_input_request(
        &self,
        input_request: &Value,
    ) -> Result<ConversationContent, AppError> {
        let question = input_request
            .get("question")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::Internal("Missing question in user input request".to_string())
            })?;

        let context = input_request
            .get("context")
            .and_then(|v| v.as_str())
            .unwrap_or("Additional information needed");

        let input_type = input_request
            .get("input_type")
            .and_then(|v| v.as_str())
            .unwrap_or("text")
            .to_string();
        let options = input_request
            .get("options")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            });
        let default_value = input_request
            .get("default_value")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let placeholder = input_request
            .get("placeholder")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(ConversationContent::UserInputRequest {
            question: question.to_string(),
            context: context.to_string(),
            input_type,
            options,
            default_value,
            placeholder,
        })
    }

    /// Strong LLM - Used for step decisions (requires good reasoning)
    pub(super) fn create_strong_llm(&self) -> Result<GeminiClient, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_else(|_| "test-key".to_string());
        Ok(GeminiClient::new(
            api_key,
            Some("gemini-3-flash-preview".to_string()),
        ).with_billing(self.agent.deployment_id, self.app_state.redis_client.clone()))
    }

    /// Weak LLM - Used for simple tasks (parameter generation, summaries, etc.)
    pub(super) fn create_weak_llm(&self) -> Result<GeminiClient, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_else(|_| "test-key".to_string());
        Ok(GeminiClient::new(
            api_key,
            Some("gemini-2.5-flash".to_string()),
        ).with_billing(self.agent.deployment_id, self.app_state.redis_client.clone()))
    }

    /// Reasoning LLM - Used for complex reasoning tasks with extended thinking
    pub(super) fn create_reasoning_llm(&self) -> Result<GeminiClient, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_else(|_| "test-key".to_string());
        Ok(GeminiClient::new(
            api_key,
            Some("gemini-3-pro-preview".to_string()),
        ).with_billing(self.agent.deployment_id, self.app_state.redis_client.clone()))
    }

    /// Execute deep reasoning using the reasoning LLM with extended thinking budget
    async fn execute_deep_reasoning(
        &self,
        directive: &DeepReasoningDirective,
    ) -> Result<(DeepReasoningResult, Option<String>), AppError> {
        let context = serde_json::json!({
            "agent_name": self.agent.name,
            "agent_description": self.agent.description,
            "problem_statement": directive.problem_statement,
            "context_summary": directive.context_summary,
            "expected_output_type": format!("{:?}", directive.expected_output_type).to_lowercase(),
            "conversation_history": self.get_conversation_history_for_llm().await,
        });

        let request_body = render_template_with_prompt(AgentTemplates::DEEP_REASONING, context)
            .map_err(|e| {
                AppError::Internal(format!("Failed to render deep reasoning template: {e}"))
            })?;

        self.create_reasoning_llm()?
            .generate_structured_content::<DeepReasoningResult>(request_body)
            .await
    }
}

/// Result from deep reasoning analysis
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DeepReasoningResult {
    pub analysis: String,
    pub conclusion: String,
    pub next_actions: Vec<String>,
    pub confidence: f64,
    #[serde(default)]
    pub caveats: Vec<String>,
}

