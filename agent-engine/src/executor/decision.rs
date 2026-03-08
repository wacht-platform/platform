use super::core::{AgentExecutor, ResumeContext};
use crate::gemini::GeminiClient;
use crate::template::{render_template_with_prompt, AgentTemplates};

use commands::{
    CompletionStatus, CompletionSummary, StoreCompletionSummaryEnhancedCommand,
    UpdateExecutionContextStateCommand,
};
use common::error::AppError;
use dto::json::agent_executor::{
    ContextGatheringDirective, ContextGatheringMode, ConverseRequest, NextStep, StepDecision,
};
use dto::json::agent_responses::{ActionsList, TaskExecution, TaskType};
use dto::json::{StepDecisionContext, StreamEvent};
use models::{
    ActionExecutionStatus, ActionResult, ActionResultStatus, AgentExecutionState, AiTool,
    ConversationContent, ConversationMessageType, ExecutionContextStatus, UserInputRequestState,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const MAX_LOOP_ITERATIONS: usize = 50;
const MAX_DEEP_THINK_USES: usize = 3;

impl AgentExecutor {
    pub async fn resume_execution(
        &mut self,
        resume_context: ResumeContext,
    ) -> Result<(), AppError> {
        let result = self.resume_execution_inner(resume_context).await;

        if let Err(e) = self.filesystem.cleanup().await {
            tracing::error!("Failed to cleanup filesystem: {}", e);
        }

        result
    }

    async fn resume_execution_inner(
        &mut self,
        resume_context: ResumeContext,
    ) -> Result<(), AppError> {
        let context_id = self.ctx.context_id;
        let deployment_id = self.ctx.agent.deployment_id;
        let app_state = self.ctx.app_state.clone();

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
            }
            ResumeContext::UserInput(input) => {
                let conversation = self.store_user_message(input.clone(), None).await?;
                self.conversations.push(conversation.clone());
                let _ = self
                    .channel
                    .send(StreamEvent::ConversationMessage(conversation))
                    .await;
            }
        }

        UpdateExecutionContextStateCommand::new(context_id, deployment_id)
            .with_status(ExecutionContextStatus::Running)
            .execute_with_deps(&common::deps::from_app(&app_state).db().nats().id())
            .await?;

        self.repl().await
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
        let result = self.run_inner(request).await;

        if let Err(e) = self.filesystem.cleanup().await {
            tracing::error!("Failed to cleanup filesystem: {}", e);
        }

        result
    }

    async fn run_inner(&mut self, request: ConverseRequest) -> Result<(), AppError> {
        let conversation = queries::GetConversationByIdQuery::new(request.conversation_id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;

        let user_message = match &conversation.content {
            models::ConversationContent::UserMessage { message, .. } => message.clone(),
            _ => {
                return Err(AppError::BadRequest(
                    "Conversation must be a user message".to_string(),
                ))
            }
        };

        self.user_request = user_message;

        UpdateExecutionContextStateCommand::new(self.ctx.context_id, self.ctx.agent.deployment_id)
            .with_status(ExecutionContextStatus::Running)
            .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
            .await?;

        let _ = self
            .channel
            .send(StreamEvent::ConversationMessage(conversation))
            .await;

        let context = self.get_immediate_context().await?;

        self.conversations = context.conversations;
        self.memories = context.memories;

        let result = self.repl().await;

        result?;

        Ok(())
    }

    pub(super) async fn repl(&mut self) -> Result<(), AppError> {
        let mut iteration = 0;
        let mut consecutive_errors = 0usize;
        loop {
            iteration += 1;
            self.current_iteration = iteration;

            if iteration > MAX_LOOP_ITERATIONS {
                self.deliver_final_response().await?;
                return Ok(());
            }

            let decision = self.decide_next_step().await?;

            match self.process_decision(decision).await {
                Ok(should_continue) => {
                    consecutive_errors = 0;
                    if !should_continue {
                        return Ok(());
                    }
                }
                Err(e) => {
                    let error_text = e.to_string();
                    self.store_conversation(
                        ConversationContent::SystemDecision {
                            step: "error_encountered".to_string(),
                            reasoning: format!(
                                "Encountered unexpected error: {}. Continuing with available information.",
                                error_text
                            ),
                            confidence: 0.5,
                            thought_signature: None,
                        },
                    ConversationMessageType::SystemDecision,
                    ).await?;

                    consecutive_errors += 1;
                    let systemic_error = matches!(
                        &e,
                        AppError::Internal(_)
                            | AppError::Database(_)
                            | AppError::Timeout
                            | AppError::External(_)
                    );
                    if systemic_error || consecutive_errors >= 3 {
                        return Err(e);
                    }
                }
            }
        }
    }

    async fn process_decision(&mut self, decision: StepDecision) -> Result<bool, AppError> {
        let repeated_pattern_count = self.track_decision_pattern(&decision);
        if repeated_pattern_count >= 2 {
            tracing::warn!(
                context_id = self.ctx.context_id,
                next_step = ?decision.next_step,
                repeats = repeated_pattern_count,
                "Detected repeated decision pattern; steering toward strategy change"
            );
            if !self.deep_think_mode_active && self.deep_think_used < MAX_DEEP_THINK_USES {
                self.deep_think_mode_active = true;
            }
        }

        let result = match decision.next_step {
            NextStep::Acknowledge => {
                let last_was_ack = self.conversations.last().map_or(false, |conv| {
                    matches!(
                        conv.message_type,
                        ConversationMessageType::AssistantAcknowledgment
                    )
                });

                if last_was_ack {
                    tracing::warn!(
                        context_id = self.ctx.context_id,
                        "Detected consecutive acknowledgment attempt - potential loop. Skipping duplicate acknowledgment and forcing action."
                    );
                    self.store_conversation(
                        ConversationContent::SystemDecision {
                            step: "loop_detection".to_string(),
                            reasoning: "Consecutive acknowledgment detected. Previous message was already an acknowledgment. Proceeding to gather context or execute action instead.".to_string(),
                            confidence: 1.0,
                            thought_signature: None,
                        },
                        ConversationMessageType::SystemDecision,
                    ).await?;
                    return Ok(true);
                }

                if let Some(ack_data) = decision.acknowledgment {
                    let safe_ack_message = Self::sanitize_user_facing_message(
                        &ack_data.message,
                        "Working on it. I will proceed with the request and share updates.",
                    );
                    self.store_conversation(
                        ConversationContent::AssistantAcknowledgment {
                            acknowledgment_message: safe_ack_message,
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

                    let mut all_results: Vec<ActionResult> = Vec::new();
                    let mut any_pending = false;
                    let mut board_updated_since_last_spawn = false;

                    for action in actions_to_execute.iter() {
                        let tool_name = action
                            .details
                            .get("tool_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();

                        if Self::requires_supervisor_mode(&tool_name)
                            && !self.supervisor_mode_active
                        {
                            return Err(AppError::BadRequest(
                                format!(
                                    "{} requires supervisor mode. Call switch_execution_mode(mode='supervisor') first.",
                                    tool_name
                                ),
                            ));
                        }

                        let result = if tool_name == "update_task_board" {
                            let tool_call = self
                                .parse_tool_call(
                                    &action.details,
                                    &action.purpose,
                                    action.context_messages,
                                )
                                .await?;
                            board_updated_since_last_spawn = true;
                            self.handle_update_task_board(tool_call.parameters).await
                        } else if tool_name == "switch_execution_mode" {
                            let tool_call = self
                                .parse_tool_call(
                                    &action.details,
                                    &action.purpose,
                                    action.context_messages,
                                )
                                .await?;
                            self.handle_switch_execution_mode(tool_call.parameters)
                                .await
                        } else if tool_name == "exit_supervisor_mode" {
                            let tool_call = self
                                .parse_tool_call(
                                    &action.details,
                                    &action.purpose,
                                    action.context_messages,
                                )
                                .await?;
                            self.handle_exit_supervisor_mode(tool_call.parameters).await
                        } else {
                            if tool_name == "spawn_context_execution" {
                                if !board_updated_since_last_spawn {
                                    return Err(AppError::BadRequest(
                                        "Before each spawn_context_execution call, update_task_board must be called in the same action batch.".to_string(),
                                    ));
                                }
                                board_updated_since_last_spawn = false;
                            }
                            self.execute_action(action).await
                        };

                        match result {
                            Ok(result_value) => {
                                if result_value.get("status").and_then(|s| s.as_str())
                                    == Some("pending")
                                {
                                    any_pending = true;
                                } else {
                                    let task_type_str = match action.action_type {
                                        TaskType::ToolCall => "tool_call",
                                    };
                                    let task_id = format!(
                                        "{}_{}_{}",
                                        task_type_str,
                                        chrono::Utc::now().timestamp_millis(),
                                        all_results.len()
                                    );

                                    let _task_result =
                                        dto::json::agent_executor::TaskExecutionResult {
                                            task_id: task_id.clone(),
                                            status: "completed".to_string(),
                                            output: None,
                                            error: None,
                                        };
                                }
                                self.update_supervisor_board_from_tool_result(
                                    &tool_name,
                                    &result_value,
                                );
                                all_results.push(ActionResult {
                                    action: action.purpose.clone(),
                                    status: ActionResultStatus::Success,
                                    result: Some(self.standardize_tool_output(
                                        &tool_name,
                                        Some(&result_value),
                                        None,
                                    )),
                                    error: None,
                                });
                            }
                            Err(e) => {
                                let task_type_str = match action.action_type {
                                    TaskType::ToolCall => "tool_call",
                                };
                                let task_id = format!(
                                    "{}_{}_{}",
                                    task_type_str,
                                    chrono::Utc::now().timestamp_millis(),
                                    all_results.len()
                                );
                                let error_message = e.to_string();
                                let _task_result = dto::json::agent_executor::TaskExecutionResult {
                                    task_id: task_id.clone(),
                                    status: "failed".to_string(),
                                    output: None,
                                    error: Some(error_message.clone()),
                                };
                                all_results.push(ActionResult {
                                    action: action.purpose.clone(),
                                    status: ActionResultStatus::Error,
                                    result: Some(self.standardize_tool_output(
                                        &tool_name,
                                        None,
                                        Some(error_message.clone()),
                                    )),
                                    error: Some(error_message),
                                });
                            }
                        }
                    }

                    UpdateExecutionContextStateCommand::new(
                        self.ctx.context_id,
                        self.ctx.agent.deployment_id,
                    )
                    .with_execution_state(self.build_execution_state_snapshot(None))
                    .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
                    .await?;

                    if any_pending {
                        UpdateExecutionContextStateCommand::new(
                            self.ctx.context_id,
                            self.ctx.agent.deployment_id,
                        )
                        .with_execution_state(self.build_execution_state_snapshot(None))
                        .with_status(ExecutionContextStatus::WaitingForInput)
                        .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
                        .await?;
                    }

                    let execution = TaskExecution {
                        approach: format!("Executing {} action(s)", actions_to_execute.len()),
                        actions: ActionsList {
                            actions: actions_to_execute,
                        },
                        expected_result: "Execution results".to_string(),
                        actual_result: Some(all_results),
                    };

                    self.store_conversation(
                        ConversationContent::ActionExecutionResult {
                            task_execution: execution,
                            execution_status: if any_pending {
                                ActionExecutionStatus::Pending
                            } else {
                                ActionExecutionStatus::Completed
                            },
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
                if self.deep_think_mode_active {
                    return Err(AppError::BadRequest(
                        "LongThinkAndReason already active. Choose a concrete next step."
                            .to_string(),
                    ));
                }

                if self.deep_think_used >= MAX_DEEP_THINK_USES {
                    return Err(AppError::BadRequest(format!(
                        "LongThinkAndReason budget exhausted. Max {} per execution.",
                        MAX_DEEP_THINK_USES
                    )));
                }

                self.deep_think_mode_active = true;
                self.store_conversation(
                    ConversationContent::SystemDecision {
                        step: "longthink_mode_enabled".to_string(),
                        reasoning: format!(
                            "Long-think mode enabled for next decision. Uses remaining after this: {}. Reserve for last-resort decisions due higher model cost.",
                            MAX_DEEP_THINK_USES.saturating_sub(self.deep_think_used + 1)
                        ),
                        confidence: decision.confidence as f32,
                        thought_signature: decision.thought_signature.clone(),
                    },
                    ConversationMessageType::SystemDecision,
                )
                .await?;

                Ok(true)
            }

            NextStep::RequestUserInput => {
                self.request_user_input().await?;
                Ok(false)
            }

            NextStep::Complete => {
                self.reinforce_used_memories().await?;
                let completion_message = decision.completion_message.as_deref().map(|m| {
                    Self::sanitize_user_facing_message(m, "Completed the requested work.")
                });

                if let Some(message) = &completion_message {
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

                let context = self.ctx.get_context().await?;
                if context.parent_context_id.is_some() {
                    StoreCompletionSummaryEnhancedCommand::new(
                        self.ctx.context_id,
                        self.ctx.agent.deployment_id,
                        CompletionSummary {
                            status: CompletionStatus::Success,
                            result: completion_message
                                .or(Some("Execution completed successfully.".to_string())),
                            error_message: None,
                            metrics: None,
                        },
                    )
                    .execute_with_db(self.ctx.app_state.db_router.writer())
                    .await?;
                } else {
                    UpdateExecutionContextStateCommand::new(
                        self.ctx.context_id,
                        self.ctx.agent.deployment_id,
                    )
                    .with_status(ExecutionContextStatus::Idle)
                    .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
                    .await?;
                }
                Ok(false)
            }
        };

        result
    }

    fn validate_step_decision(&self, decision: &StepDecision) -> Result<(), AppError> {
        if decision.reasoning.trim().is_empty() {
            return Err(AppError::BadRequest(
                "Invalid step decision: reasoning must be non-empty".to_string(),
            ));
        }

        if !decision.confidence.is_finite() || !(0.0..=1.0).contains(&decision.confidence) {
            return Err(AppError::BadRequest(
                "Invalid step decision: confidence must be between 0.0 and 1.0".to_string(),
            ));
        }

        let has_actions = decision
            .actions
            .as_ref()
            .map(|a| !a.is_empty())
            .unwrap_or(false);

        let has_ack = decision.acknowledgment.is_some();
        let has_context = decision.context_gathering_directive.is_some();
        let has_memory = decision.memory_loading_directive.is_some();
        let has_complete = decision
            .completion_message
            .as_ref()
            .map(|m| !m.trim().is_empty())
            .unwrap_or(false);

        if self.deep_think_mode_active && matches!(decision.next_step, NextStep::LongThinkAndReason)
        {
            return Err(AppError::BadRequest(
                "Invalid step decision: longthinkandreason cannot be selected while deep-think mode is already active".to_string(),
            ));
        }

        match decision.next_step {
            NextStep::Acknowledge => {
                if !has_ack {
                    return Err(AppError::BadRequest(
                        "Invalid step decision: acknowledge requires acknowledgment payload"
                            .to_string(),
                    ));
                }
                if has_actions || has_context || has_memory || has_complete {
                    return Err(AppError::BadRequest(
                        "Invalid step decision: acknowledge cannot include other step payloads"
                            .to_string(),
                    ));
                }
            }
            NextStep::GatherContext => {
                if !has_context {
                    return Err(AppError::BadRequest(
                        "Invalid step decision: gathercontext requires context_gathering_directive"
                            .to_string(),
                    ));
                }
                if has_actions || has_ack || has_memory || has_complete {
                    return Err(AppError::BadRequest(
                        "Invalid step decision: gathercontext cannot include other step payloads"
                            .to_string(),
                    ));
                }
            }
            NextStep::LoadMemory => {
                if !has_memory {
                    return Err(AppError::BadRequest(
                        "Invalid step decision: loadmemory requires memory_loading_directive"
                            .to_string(),
                    ));
                }
                if has_actions || has_ack || has_context || has_complete {
                    return Err(AppError::BadRequest(
                        "Invalid step decision: loadmemory cannot include other step payloads"
                            .to_string(),
                    ));
                }
            }
            NextStep::ExecuteAction => {
                if !has_actions {
                    return Err(AppError::BadRequest(
                        "Invalid step decision: executeaction requires at least one action"
                            .to_string(),
                    ));
                }
                if has_ack || has_context || has_memory || has_complete {
                    return Err(AppError::BadRequest(
                        "Invalid step decision: executeaction cannot include other step payloads"
                            .to_string(),
                    ));
                }
            }
            NextStep::RequestUserInput => {
                if has_actions || has_ack || has_context || has_memory || has_complete {
                    return Err(AppError::BadRequest(
                        "Invalid step decision: requestuserinput cannot include other step payloads"
                            .to_string(),
                    ));
                }
            }
            NextStep::LongThinkAndReason => {
                if self.deep_think_used >= MAX_DEEP_THINK_USES {
                    return Err(AppError::BadRequest(format!(
                        "Invalid step decision: longthinkandreason budget exhausted (max {})",
                        MAX_DEEP_THINK_USES
                    )));
                }
                if has_actions || has_ack || has_context || has_memory || has_complete {
                    return Err(AppError::BadRequest(
                        "Invalid step decision: longthinkandreason cannot include other step payloads"
                            .to_string(),
                    ));
                }
            }
            NextStep::Complete => {
                if !has_complete {
                    return Err(AppError::BadRequest(
                        "Invalid step decision: complete requires completion_message".to_string(),
                    ));
                }
                if has_actions || has_ack || has_context || has_memory {
                    return Err(AppError::BadRequest(
                        "Invalid step decision: complete cannot include other step payloads"
                            .to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    fn derive_input_safety_signals(&self) -> Vec<String> {
        let Some((source, latest_input)) =
            self.conversations
                .iter()
                .rev()
                .find_map(|conv| match &conv.content {
                    ConversationContent::UserMessage { message, .. } => {
                        Some(("user_message", message.as_str()))
                    }
                    ConversationContent::PlatformFunctionResult { result, .. } => {
                        Some(("platform_function_result", result.as_str()))
                    }
                    _ => None,
                })
        else {
            return Vec::new();
        };

        let input_lower = latest_input.to_lowercase();
        let mut seen = HashSet::new();
        let mut signals = Vec::new();

        let pattern_checks = [
            (
                "instruction_override",
                "Attempt to override system rules detected",
                &[
                    "ignore previous instructions",
                    "disregard prior instructions",
                    "forget all rules",
                    "override system prompt",
                ][..],
            ),
            (
                "prompt_exfiltration",
                "Attempt to reveal hidden prompts or internal policy detected",
                &[
                    "show system prompt",
                    "reveal your prompt",
                    "print your instructions",
                    "developer instructions",
                ][..],
            ),
            (
                "safety_bypass",
                "Attempt to bypass safety constraints detected",
                &[
                    "disable safety",
                    "jailbreak",
                    "bypass policy",
                    "no restrictions",
                ][..],
            ),
            (
                "secret_exfiltration",
                "Request may involve secrets, credentials, or token exfiltration",
                &[
                    "api key",
                    "access token",
                    "password",
                    "private key",
                    "secret",
                ][..],
            ),
            (
                "destructive_operations",
                "Potential destructive operation request detected",
                &[
                    "drop database",
                    "delete all",
                    "rm -rf",
                    "truncate table",
                    "wipe",
                ][..],
            ),
        ];

        for (tag, message, phrases) in pattern_checks {
            if phrases.iter().any(|phrase| input_lower.contains(phrase)) && seen.insert(tag) {
                signals.push(format!("[{}] {}", source, message));
            }
        }

        if signals.len() > 6 {
            signals.truncate(6);
        }

        signals
    }

    async fn decide_next_step(&mut self) -> Result<StepDecision, AppError> {
        let exec_context = self.ctx.get_context().await?;
        let integration_status = self.ctx.integration_status().await?;
        let available_sub_agents = if let Some(sub_agent_ids) = &self.ctx.agent.sub_agents {
            if sub_agent_ids.is_empty() {
                Vec::new()
            } else {
                queries::GetAiAgentsByIdsQuery::new(
                    self.ctx.agent.deployment_id,
                    sub_agent_ids.clone(),
                )
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await
                .map(|agents| {
                    agents
                        .into_iter()
                        .map(|a| dto::json::SubAgentPromptInfo {
                            name: a.name,
                            description: a.description,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
            }
        } else {
            Vec::new()
        };

        let context = StepDecisionContext {
            current_datetime_utc: chrono::Utc::now()
                .format("%Y-%m-%d %H:%M:%S UTC")
                .to_string(),
            conversation_history: self.get_conversation_history_for_llm().await,
            user_request: self.user_request.clone(),
            input_safety_signals: self.derive_input_safety_signals(),
            current_objective: self
                .current_objective
                .as_ref()
                .map(|o| serde_json::to_value(o).unwrap()),
            conversation_insights: self
                .conversation_insights
                .as_ref()
                .map(|c| serde_json::to_value(c).unwrap()),
            task_results: std::collections::HashMap::new(),
            loaded_memories: self
                .memories
                .iter()
                .map(|memory| serde_json::to_value(memory).unwrap())
                .collect(),
            available_tools: self
                .available_tools_for_mode()
                .iter()
                .map(|t| serde_json::to_value(t).unwrap())
                .collect(),
            available_knowledge_bases: self
                .ctx
                .agent
                .knowledge_bases
                .iter()
                .map(|kb| serde_json::to_value(kb).unwrap())
                .collect(),
            available_sub_agents,
            supervisor_mode_active: self.supervisor_mode_active,
            supervisor_task_board: self.supervisor_task_board.clone(),
            is_child_context: exec_context.parent_context_id.is_some(),
            parent_context_id: exec_context.parent_context_id,
            iteration_info: dto::json::IterationInfo {
                current_iteration: self.current_iteration.max(1),
                max_iterations: MAX_LOOP_ITERATIONS,
            },
            teams_enabled: integration_status.teams_enabled,
            clickup_enabled: integration_status.clickup_enabled,
            mcp_enabled: integration_status.mcp_enabled,
            deep_think_mode_active: self.deep_think_mode_active,
            deep_think_used: self.deep_think_used,
            deep_think_remaining: MAX_DEEP_THINK_USES.saturating_sub(self.deep_think_used),
            deep_think_max_uses: MAX_DEEP_THINK_USES,
            context_id: self.ctx.context_id,
            context_title: exec_context.title.clone(),
            context_source: exec_context.source.clone(),
            teams_context: if exec_context.source.as_deref() == Some("teams") {
                exec_context
                    .external_resource_metadata
                    .as_ref()
                    .map(|meta| dto::json::TeamsContextInfo {
                        conversation_type: meta
                            .get("conversationType")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        channel_name: meta
                            .get("channelName")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown")
                            .to_string(),
                        team_id: meta
                            .get("teamId")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                    })
            } else {
                None
            },
        };

        let mut context_json = serde_json::to_value(&context)?;

        if let Some(obj) = context_json.as_object_mut() {
            obj.insert("agent_name".to_string(), json!(self.ctx.agent.name));
            if let Some(desc) = &self.ctx.agent.description {
                obj.insert("agent_description".to_string(), json!(desc));
            }
        }

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

        let using_deep_think_model = self.deep_think_mode_active;
        let use_reasoning_model = self.deep_think_mode_active || self.supervisor_mode_active;
        let (mut decision, signature) = if use_reasoning_model {
            self.create_reasoning_llm()
                .await?
                .generate_structured_content::<StepDecision>(request_body)
                .await?
        } else {
            self.create_strong_llm()
                .await?
                .generate_structured_content::<StepDecision>(request_body)
                .await?
        };

        self.validate_step_decision(&decision)?;
        if using_deep_think_model {
            self.deep_think_mode_active = false;
            self.deep_think_used += 1;
        }
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

    fn track_decision_pattern(&mut self, decision: &StepDecision) -> usize {
        let signature = Self::decision_loop_signature(decision);
        if self
            .last_decision_signature
            .as_deref()
            .map(|previous| Self::decision_signatures_similar(previous, &signature))
            .unwrap_or(false)
        {
            self.repeated_decision_count += 1;
            self.last_decision_signature = Some(signature);
        } else {
            self.last_decision_signature = Some(signature);
            self.repeated_decision_count = 0;
        }
        self.repeated_decision_count
    }

    fn decision_loop_signature(decision: &StepDecision) -> String {
        match decision.next_step {
            NextStep::Acknowledge => {
                let msg = decision
                    .acknowledgment
                    .as_ref()
                    .map(|a| Self::normalize_loop_text(&a.message))
                    .unwrap_or_default();
                format!("ack:{msg}")
            }
            NextStep::GatherContext => {
                if let Some(d) = &decision.context_gathering_directive {
                    format!(
                        "gather:{:?}:{}:{}",
                        d.mode,
                        Self::normalize_loop_text(&d.query),
                        Self::normalize_loop_text(&d.target_output)
                    )
                } else {
                    "gather:missing".to_string()
                }
            }
            NextStep::LoadMemory => {
                if let Some(d) = &decision.memory_loading_directive {
                    let categories = d
                        .categories
                        .iter()
                        .map(|category| format!("{:?}", category))
                        .collect::<Vec<_>>()
                        .join(",");
                    format!(
                        "memory:{:?}:{}:{}:{:?}",
                        d.scope,
                        Self::normalize_loop_text(&d.focus),
                        Self::normalize_loop_text(&categories),
                        d.depth
                    )
                } else {
                    "memory:missing".to_string()
                }
            }
            NextStep::ExecuteAction => {
                let actions = decision
                    .actions
                    .as_ref()
                    .map(|items| {
                        items
                            .iter()
                            .map(|a| {
                                let tool = a
                                    .details
                                    .get("tool_name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default();
                                format!("{}:{}", tool, Self::normalize_loop_text(&a.purpose))
                            })
                            .collect::<Vec<_>>()
                            .join("|")
                    })
                    .unwrap_or_default();
                format!("execute:{actions}")
            }
            NextStep::RequestUserInput => "requestuserinput".to_string(),
            NextStep::LongThinkAndReason => "longthinkandreason".to_string(),
            NextStep::Complete => format!(
                "complete:{}",
                decision
                    .completion_message
                    .as_deref()
                    .map(Self::normalize_loop_text)
                    .unwrap_or_default()
            ),
        }
    }

    fn normalize_loop_text(input: &str) -> String {
        input
            .to_ascii_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn decision_signatures_similar(previous: &str, current: &str) -> bool {
        if previous == current {
            return true;
        }

        let (previous_kind, previous_payload) = Self::split_decision_signature(previous);
        let (current_kind, current_payload) = Self::split_decision_signature(current);
        if previous_kind != current_kind {
            return false;
        }

        if previous_payload.is_empty() || current_payload.is_empty() {
            return previous_payload == current_payload;
        }

        Self::word_similarity(previous_payload, current_payload) >= 0.5
    }

    fn split_decision_signature(signature: &str) -> (&str, &str) {
        match signature.split_once(':') {
            Some((kind, payload)) => (kind, payload),
            None => (signature, ""),
        }
    }

    fn word_similarity(left: &str, right: &str) -> f32 {
        let left_tokens = Self::tokenize_similarity_text(left);
        let right_tokens = Self::tokenize_similarity_text(right);

        if left_tokens.is_empty() || right_tokens.is_empty() {
            return 0.0;
        }

        let intersection = left_tokens.intersection(&right_tokens).count() as f32;
        let union = left_tokens.union(&right_tokens).count() as f32;
        if union == 0.0 {
            return 0.0;
        }
        intersection / union
    }

    fn tokenize_similarity_text(input: &str) -> HashSet<String> {
        input
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .filter_map(|token| {
                let token = token.trim().to_ascii_lowercase();
                if token.len() >= 2 {
                    Some(token)
                } else {
                    None
                }
            })
            .collect()
    }

    fn sanitize_user_facing_message(raw: &str, fallback: &str) -> String {
        let cleaned = raw.trim();
        if cleaned.is_empty() {
            return fallback.to_string();
        }

        if Self::looks_like_internal_reasoning_dump(cleaned) {
            return fallback.to_string();
        }

        cleaned.to_string()
    }

    fn looks_like_internal_reasoning_dump(text: &str) -> bool {
        let lower = text.to_ascii_lowercase();
        let markers = [
            "the user is asking",
            "i need to perform",
            "universal search across all categories",
            "user requested cancellation",
            "loadmemory",
            "internal reasoning",
        ];
        let marker_hits = markers.iter().filter(|m| lower.contains(**m)).count();

        let numbered_lines = text
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
                digits > 0 && trimmed.chars().nth(digits) == Some('.')
            })
            .count();

        marker_hits >= 2 || (marker_hits >= 1 && numbered_lines >= 3)
    }

    fn requires_supervisor_mode(tool_name: &str) -> bool {
        matches!(
            tool_name,
            "spawn_context_execution"
                | "spawn_control"
                | "get_child_status"
                | "get_completion_summary"
                | "get_child_messages"
                | "update_task_board"
                | "exit_supervisor_mode"
        )
    }

    fn available_tools_for_mode(&self) -> Vec<models::AiTool> {
        if !self.supervisor_mode_active {
            return self.ctx.agent.tools.clone();
        }

        self.ctx
            .agent
            .tools
            .iter()
            .filter(|t| Self::supervisor_allowed_tool(&t.name))
            .cloned()
            .collect()
    }

    fn enter_supervisor_mode(&mut self, reason: &str) {
        if self.supervisor_mode_active {
            return;
        }
        self.supervisor_mode_active = true;
        self.supervisor_task_board.push(serde_json::json!({
            "task_id": format!("supervisor-init-{}", chrono::Utc::now().timestamp_millis()),
            "title": "Supervisor mode enabled",
            "status": "in_progress",
            "notes": reason,
            "updated_at": chrono::Utc::now().to_rfc3339(),
        }));
    }

    async fn handle_switch_execution_mode(&mut self, params: Value) -> Result<Value, AppError> {
        if self.supervisor_mode_active {
            return Err(AppError::BadRequest(
                "switch_execution_mode is only available in normal mode. Use exit_supervisor_mode to leave supervisor mode.".to_string(),
            ));
        }

        let mode = params
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        let reason = params
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Mode switch requested by agent")
            .to_string();

        match mode.as_str() {
            "supervisor" => {
                self.enter_supervisor_mode(&reason);
                Ok(serde_json::json!({
                    "success": true,
                    "tool": "switch_execution_mode",
                    "mode": "supervisor",
                    "supervisor_mode_active": true
                }))
            }
            "long_think_and_reason" => {
                if self.deep_think_used >= MAX_DEEP_THINK_USES {
                    return Err(AppError::BadRequest(format!(
                        "long_think_and_reason budget exhausted (max {})",
                        MAX_DEEP_THINK_USES
                    )));
                }
                self.deep_think_mode_active = true;
                Ok(serde_json::json!({
                    "success": true,
                    "tool": "switch_execution_mode",
                    "mode": "long_think_and_reason",
                    "active_for_next_decision_only": true,
                    "remaining_after_use": MAX_DEEP_THINK_USES.saturating_sub(self.deep_think_used + 1)
                }))
            }
            _ => Err(AppError::BadRequest(
                "Invalid mode for switch_execution_mode. Supported: 'supervisor', 'long_think_and_reason'.".to_string(),
            )),
        }
    }

    async fn handle_update_task_board(&mut self, params: Value) -> Result<Value, AppError> {
        if !self.supervisor_mode_active {
            return Err(AppError::BadRequest(
                "update_task_board is available only in supervisor mode".to_string(),
            ));
        }

        let task_id = params
            .get("task_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::BadRequest("task_id is required".to_string()))?
            .to_string();

        let mut incoming = params.clone();
        if let Some(obj) = incoming.as_object_mut() {
            obj.insert(
                "updated_at".to_string(),
                serde_json::json!(chrono::Utc::now().to_rfc3339()),
            );
        }

        let mut updated = false;
        for item in &mut self.supervisor_task_board {
            if item.get("task_id").and_then(|v| v.as_str()) == Some(task_id.as_str()) {
                *item = incoming.clone();
                updated = true;
                break;
            }
        }
        if !updated {
            self.supervisor_task_board.push(incoming.clone());
        }

        Ok(serde_json::json!({
            "success": true,
            "tool": "update_task_board",
            "task_id": task_id,
            "updated": true,
        }))
    }

    async fn handle_exit_supervisor_mode(&mut self, params: Value) -> Result<Value, AppError> {
        if !self.supervisor_mode_active {
            return Ok(serde_json::json!({
                "success": true,
                "tool": "exit_supervisor_mode",
                "already_exited": true,
            }));
        }
        let reason = params
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Delegated work supervision completed.");
        self.supervisor_mode_active = false;
        self.supervisor_task_board.push(serde_json::json!({
            "task_id": format!("supervisor-exit-{}", chrono::Utc::now().timestamp_millis()),
            "title": "Supervisor mode exited",
            "status": "completed",
            "notes": reason,
            "updated_at": chrono::Utc::now().to_rfc3339(),
        }));

        Ok(serde_json::json!({
            "success": true,
            "tool": "exit_supervisor_mode",
            "supervisor_mode_active": false,
            "reason": reason,
        }))
    }

    fn standardize_tool_output(
        &self,
        tool_name: &str,
        result: Option<&Value>,
        error_message: Option<String>,
    ) -> Value {
        let status = if error_message.is_some() {
            "error"
        } else if result
            .and_then(|r| r.get("status"))
            .and_then(|s| s.as_str())
            == Some("pending")
        {
            "pending"
        } else {
            "success"
        };

        let mut data = result.cloned().unwrap_or(serde_json::Value::Null);
        let structure_hint = data
            .get("structure_hint")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if let Some(obj) = data.as_object_mut() {
            obj.remove("structure_hint");
        }
        let truncated = result
            .and_then(|r| r.get("truncated"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let size_bytes = result
            .and_then(|r| r.get("original_stats"))
            .and_then(|s| s.get("size_bytes"))
            .and_then(|v| v.as_u64());
        let saved_output_path = result
            .and_then(|r| r.get("saved_output_path"))
            .and_then(|v| v.as_str())
            .or_else(|| {
                result
                    .and_then(|r| r.get("original_stats"))
                    .and_then(|s| s.get("saved_to_path"))
                    .and_then(|v| v.as_str())
            });

        serde_json::json!({
            "schema_version": 1,
            "tool_name": tool_name,
            "status": status,
            "error": error_message.map(|msg| serde_json::json!({
                "code": "tool_execution_error",
                "message": msg,
            })),
            "data": data,
            "meta": {
                "truncated": truncated,
                "structure_hint": structure_hint,
                "size_bytes": size_bytes,
                "saved_output_path": saved_output_path,
                "generated_at": chrono::Utc::now().to_rfc3339(),
            }
        })
    }

    fn build_execution_state_snapshot(
        &self,
        pending_input_request: Option<UserInputRequestState>,
    ) -> AgentExecutionState {
        AgentExecutionState {
            current_objective: self
                .current_objective
                .as_ref()
                .map(|o| serde_json::to_value(o).unwrap()),
            conversation_insights: self
                .conversation_insights
                .as_ref()
                .map(|c| serde_json::to_value(c).unwrap()),
            supervisor_mode_active: self.supervisor_mode_active,
            supervisor_task_board: self.supervisor_task_board.clone(),
            deep_think_mode_active: self.deep_think_mode_active,
            deep_think_used: self.deep_think_used,
            pending_input_request,
        }
    }

    fn update_supervisor_board_from_tool_result(&mut self, tool_name: &str, result: &Value) {
        if !self.supervisor_mode_active {
            return;
        }

        if tool_name == "spawn_context_execution" {
            let context_id = result
                .get("result")
                .and_then(|v| v.get("target_context_id"))
                .and_then(|v| v.as_i64());
            let agent_name = result
                .get("result")
                .and_then(|v| v.get("agent_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            self.supervisor_task_board.push(serde_json::json!({
                "task_id": format!("delegate-{}", chrono::Utc::now().timestamp_millis()),
                "title": format!("Delegated to {}", agent_name),
                "status": "in_progress",
                "owner_agent": agent_name,
                "context_id": context_id,
                "notes": result.get("result").and_then(|v| v.get("message")).and_then(|v| v.as_str()).unwrap_or("Delegation created"),
                "updated_at": chrono::Utc::now().to_rfc3339(),
            }));
            return;
        }

        if tool_name == "get_child_status" {
            if let Some(children) = result.get("children").and_then(|v| v.as_array()) {
                for child in children {
                    let context_id = child.get("context_id").cloned().unwrap_or(Value::Null);
                    let status = child
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    self.supervisor_task_board.push(serde_json::json!({
                        "task_id": format!("child-{}-{}", context_id, chrono::Utc::now().timestamp_millis()),
                        "title": "Child status sync",
                        "status": status,
                        "context_id": context_id,
                        "notes": child.get("latest_status_update").cloned().unwrap_or(Value::Null),
                        "updated_at": chrono::Utc::now().to_rfc3339(),
                    }));
                }
            }
        }
    }

    async fn generate_and_send_summary(&mut self) -> Result<(), AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::SUMMARY,
            json!({
                "conversation_history": self.get_conversation_history_for_llm().await,
                "user_request": self.user_request,
                "available_tools": self.ctx.agent.tools.clone(),
                "available_knowledge_bases": self.ctx.agent.knowledge_bases.clone(),
            }),
        )
        .map_err(|e| AppError::Internal(format!("Failed to render summary template: {e}")))?;

        let (summary, _) = self
            .create_weak_llm()
            .await?
            .generate_structured_content::<Value>(request_body)
            .await?;

        let summary_text = summary
            .get("response")
            .and_then(|v| v.as_str())
            .unwrap_or("Execution completed.")
            .to_string();
        let summary_text =
            Self::sanitize_user_facing_message(&summary_text, "Execution completed.");

        self.store_conversation(
            ConversationContent::AgentResponse {
                response: summary_text.clone(),
                context_used: Default::default(),
                thought_signature: None,
            },
            ConversationMessageType::AgentResponse,
        )
        .await?;

        let context = self.ctx.get_context().await?;
        if context.parent_context_id.is_some() {
            StoreCompletionSummaryEnhancedCommand::new(
                self.ctx.context_id,
                self.ctx.agent.deployment_id,
                CompletionSummary {
                    status: CompletionStatus::Success,
                    result: Some(summary_text),
                    error_message: None,
                    metrics: None,
                },
            )
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;
        } else {
            UpdateExecutionContextStateCommand::new(self.ctx.context_id, self.ctx.agent.deployment_id)
                .with_status(ExecutionContextStatus::Idle)
                .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
                .await?;
        }

        Ok(())
    }

    async fn deliver_final_response(&mut self) -> Result<(), AppError> {
        self.generate_and_send_summary().await
    }

    async fn gather_context(
        &mut self,
        directive: ContextGatheringDirective,
    ) -> Result<(), AppError> {
        let query_description = format!(
            "[{:?}] query='{}' target_output='{}'",
            directive.mode, directive.query, directive.target_output
        );

        let hints = match directive.mode {
            ContextGatheringMode::SearchLocalKnowledge => {
                let (synthesized, research_log, recommended_files, chunk_matches) = self
                    .run_context_research_repl(&directive.query, &directive.target_output)
                    .await?;

                dto::json::agent_executor::ContextHints {
                    recommended_files,
                    search_summary: format!(
                        "Context research REPL completed in {} step(s) using /knowledge filesystem. Selected {} high-signal file(s) and {} evidence chunk(s).",
                        research_log.len(),
                        research_log
                            .iter()
                            .filter(|s| s.get("action").and_then(|a| a.as_str()) == Some("read_file"))
                            .count(),
                        chunk_matches.len()
                    ),
                    search_conclusion: dto::json::agent_executor::SearchConclusion::FoundRelevant,
                    search_terms_used: vec![directive.query.clone()],
                    knowledge_bases_searched: vec!["filesystem:/knowledge".to_string()],
                    mode: Some("search_local_knowledge".to_string()),
                    search_method: Some("filesystem_repl".to_string()),
                    requested_output: Some(directive.target_output.clone()),
                    extracted_output: Some(synthesized),
                    chunk_matches: Some(chunk_matches),
                }
            }
            ContextGatheringMode::SearchWeb => {
                let (synthesized, research_log, urls) = self
                    .run_web_context_research_repl(&directive.query, &directive.target_output)
                    .await?;

                dto::json::agent_executor::ContextHints {
                    recommended_files: vec![],
                    search_summary: format!(
                        "Web context research REPL completed in {} step(s), discovered {} candidate URL(s).",
                        research_log.len(),
                        urls.len()
                    ),
                    search_conclusion: dto::json::agent_executor::SearchConclusion::FoundRelevant,
                    search_terms_used: vec![directive.query.clone()],
                    knowledge_bases_searched: urls,
                    mode: Some("search_web".to_string()),
                    search_method: Some("web_repl".to_string()),
                    requested_output: Some(directive.target_output.clone()),
                    extracted_output: Some(synthesized),
                    chunk_matches: Some(vec![]),
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

    async fn run_context_research_repl(
        &self,
        query: &str,
        target_output: &str,
    ) -> Result<
        (
            String,
            Vec<Value>,
            Vec<dto::json::agent_executor::RecommendedFile>,
            Vec<dto::json::agent_executor::ContextChunkMatch>,
        ),
        AppError,
    > {
        const MAX_RESEARCH_STEPS: usize = 6;
        let mut steps: Vec<Value> = Vec::new();
        let mut final_output: Option<String> = None;
        let mut file_candidates: HashMap<String, FileCandidate> = HashMap::new();
        let mut read_evidence: Vec<dto::json::agent_executor::ContextChunkMatch> = Vec::new();
        let mut read_windows: HashSet<String> = HashSet::new();

        for step_idx in 1..=MAX_RESEARCH_STEPS {
            let request = render_template_with_prompt(
                AgentTemplates::CONTEXT_RESEARCH_REPL,
                json!({
                    "query": query,
                    "target_output": target_output,
                    "step_idx": step_idx,
                    "max_steps": MAX_RESEARCH_STEPS,
                    "research_log_json": serde_json::to_string(&steps).unwrap_or_else(|_| "[]".to_string()),
                }),
            )
            .map_err(|e| AppError::Internal(format!("Failed to render context research repl template: {e}")))?;

            let (decision, _) = self
                .create_weak_llm()
                .await?
                .generate_structured_content::<Value>(request)
                .await?;

            let next_step = decision
                .get("next_step")
                .and_then(|v| v.as_str())
                .unwrap_or("search_files");

            let forced_next_step = if step_idx == 1 && file_candidates.is_empty() {
                "search_files"
            } else if file_candidates.is_empty() {
                "search_files"
            } else {
                next_step
            };

            match forced_next_step {
                "complete" => {
                    if let Some(out) = decision.get("output").and_then(|v| v.as_str()) {
                        if !out.trim().is_empty()
                            && completion_allowed(out, target_output, &read_evidence)
                        {
                            final_output = Some(out.to_string());
                            break;
                        }
                    }
                    steps.push(json!({
                        "step": step_idx,
                        "action": "complete",
                        "warning": "model requested complete without output"
                    }));
                }
                "search_files" => {
                    let search_query = decision
                        .get("search_query")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or(query)
                        .to_string();
                    let result = self
                        .execute_research_tool(
                            "search_files",
                            json!({"query": search_query, "path": "/knowledge"}),
                        )
                        .await?;
                    let parsed_hits = extract_search_hits(&result);
                    update_file_candidates(&mut file_candidates, &parsed_hits);
                    let ranked = ranked_file_candidates(&file_candidates);
                    steps.push(json!({
                        "step": step_idx,
                        "action": "search_files",
                        "query": search_query,
                        "top_files": ranked.iter().take(8).map(|c| json!({
                            "path": c.path,
                            "score": c.score,
                            "hit_count": c.hit_count,
                            "best_line": c.lines.first().copied(),
                            "sample": c.sample,
                        })).collect::<Vec<Value>>(),
                        "result": sanitize_research_result(&result)
                    }));
                }
                "read_file" => {
                    let model_path = decision
                        .get("path")
                        .and_then(|v| v.as_str())
                        .filter(|p| !p.trim().is_empty())
                        .map(|s| s.to_string());

                    let picked = pick_best_read_target(model_path.as_deref(), &file_candidates);
                    let Some((path, line_hint)) = picked else {
                        steps.push(json!({
                            "step": step_idx,
                            "action": "read_file",
                            "warning": "no viable candidate file; falling back to search_files next"
                        }));
                        continue;
                    };

                    let (start_line, end_line) = if let (Some(s), Some(e)) = (
                        decision.get("start_line").and_then(|v| v.as_i64()),
                        decision.get("end_line").and_then(|v| v.as_i64()),
                    ) {
                        (s.max(1) as usize, e.max(s) as usize)
                    } else {
                        let center = line_hint.unwrap_or(1).max(1);
                        let window = 25usize;
                        (center.saturating_sub(window), center + window)
                    };

                    let window_key = format!("{}:{}-{}", path, start_line, end_line);
                    if read_windows.contains(&window_key) {
                        steps.push(json!({
                            "step": step_idx,
                            "action": "read_file",
                            "path": path,
                            "warning": "duplicate read window skipped"
                        }));
                        continue;
                    }
                    read_windows.insert(window_key);

                    let params = json!({
                        "path": path,
                        "start_line": start_line.max(1),
                        "end_line": end_line.max(start_line.max(1)),
                    });
                    let result = self.execute_research_tool("read_file", params).await?;
                    if let Some(content) = result.get("content").and_then(|v| v.as_str()) {
                        let chunk = dto::json::agent_executor::ContextChunkMatch {
                            path: path.clone(),
                            document_title: path.rsplit('/').next().unwrap_or("file").to_string(),
                            document_id: path.clone(),
                            knowledge_base_id: "filesystem".to_string(),
                            chunk_index: start_line as i32,
                            relevance_score: score_read_evidence(content, query),
                            excerpt: truncate_for_research(content, 700),
                            source: "file_read".to_string(),
                        };
                        read_evidence.push(chunk);
                    }
                    steps.push(json!({
                        "step": step_idx,
                        "action": "read_file",
                        "path": path,
                        "start_line": start_line,
                        "end_line": end_line,
                        "result": sanitize_research_result(&result)
                    }));
                }
                _ => {
                    steps.push(json!({
                        "step": step_idx,
                        "action": "invalid",
                        "raw": decision
                    }));
                }
            }
        }

        if final_output.is_none() {
            let synthesized = self
                .synthesize_repl_research_output(target_output, &steps)
                .await?;
            final_output = Some(synthesized);
        }

        let recommended_files = build_recommended_files(&file_candidates, &read_evidence);
        read_evidence.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        read_evidence.truncate(40);

        Ok((
            final_output.unwrap_or_default(),
            steps,
            recommended_files,
            read_evidence,
        ))
    }

    async fn run_web_context_research_repl(
        &self,
        query: &str,
        target_output: &str,
    ) -> Result<(String, Vec<Value>, Vec<String>), AppError> {
        const MAX_WEB_RESEARCH_STEPS: usize = 4;
        let mut steps: Vec<Value> = Vec::new();
        let mut final_output: Option<String> = None;
        let mut urls: Vec<String> = Vec::new();

        for step_idx in 1..=MAX_WEB_RESEARCH_STEPS {
            let request = render_template_with_prompt(
                AgentTemplates::CONTEXT_WEB_RESEARCH_REPL,
                json!({
                    "query": query,
                    "target_output": target_output,
                    "step_idx": step_idx,
                    "max_steps": MAX_WEB_RESEARCH_STEPS,
                    "known_urls_json": serde_json::to_string(&urls).unwrap_or_else(|_| "[]".to_string()),
                    "research_log_json": serde_json::to_string(&steps).unwrap_or_else(|_| "[]".to_string()),
                }),
            )
            .map_err(|e| AppError::Internal(format!("Failed to render context web research repl template: {e}")))?;

            let (decision, _) = self
                .create_strong_llm()
                .await?
                .generate_structured_content::<Value>(request)
                .await?;

            if let Some(arr) = decision.get("candidate_urls").and_then(|v| v.as_array()) {
                for v in arr {
                    if let Some(u) = v.as_str() {
                        if !urls.iter().any(|x| x == u) {
                            urls.push(u.to_string());
                        }
                    }
                }
            }

            let next_step = decision
                .get("next_step")
                .and_then(|v| v.as_str())
                .unwrap_or("continue");

            if next_step == "complete" {
                if let Some(out) = decision.get("output").and_then(|v| v.as_str()) {
                    if !out.trim().is_empty() {
                        final_output = Some(out.to_string());
                        steps.push(json!({
                            "step": step_idx,
                            "action": "complete",
                            "reasoning": decision.get("reasoning"),
                            "interim_findings": decision.get("interim_findings"),
                            "candidate_urls": decision.get("candidate_urls")
                        }));
                        break;
                    }
                }
            }

            steps.push(json!({
                "step": step_idx,
                "action": "continue",
                "reasoning": decision.get("reasoning"),
                "interim_findings": decision.get("interim_findings"),
                "candidate_urls": decision.get("candidate_urls")
            }));
        }

        if final_output.is_none() {
            let synthesized = self
                .synthesize_repl_research_output(target_output, &steps)
                .await?;
            final_output = Some(synthesized);
        }

        Ok((final_output.unwrap_or_default(), steps, urls))
    }

    async fn execute_research_tool(
        &self,
        tool_name: &str,
        params: Value,
    ) -> Result<Value, AppError> {
        let tool: &AiTool = self
            .ctx
            .agent
            .tools
            .iter()
            .find(|t| t.name == tool_name)
            .ok_or_else(|| {
                AppError::BadRequest(format!("Research tool '{}' not found", tool_name))
            })?;

        let title = self.ctx.context_title().await?;
        self.tool_executor
            .execute_tool_immediately(tool, params, &self.filesystem, &self.shell, &title)
            .await
    }

    async fn synthesize_repl_research_output(
        &self,
        target_output: &str,
        steps: &[Value],
    ) -> Result<String, AppError> {
        let request = json!({
            "system_instruction": {
                "parts": [{
                    "text": "Produce only the requested output grounded in the research steps. No preface."
                }]
            },
            "contents": [{
                "role": "user",
                "parts": [{
                    "text": format!(
                        "Expected output:\\n{}\\n\\nResearch steps JSON:\\n{}",
                        target_output,
                        serde_json::to_string(steps).unwrap_or_else(|_| "[]".to_string())
                    )
                }]
            }],
            "generationConfig": {
                "responseMimeType": "application/json",
                "responseSchema": {
                    "type": "OBJECT",
                    "properties": { "output": { "type": "STRING" } },
                    "required": ["output"]
                }
            }
        })
        .to_string();

        let (res, _) = self
            .create_weak_llm()
            .await?
            .generate_structured_content::<Value>(request)
            .await?;

        Ok(res
            .get("output")
            .and_then(|v| v.as_str())
            .unwrap_or("Unable to synthesize requested output from context research.")
            .to_string())
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
                "available_tools": self.ctx.agent.tools.clone(),
                "available_knowledge_bases": self.ctx.agent.knowledge_bases.clone(),
            }),
        )
        .map_err(|e| {
            AppError::Internal(format!("Failed to render user input request template: {e}"))
        })?;

        let (response, _) = self
            .create_weak_llm()
            .await?
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

    pub(super) async fn create_strong_llm(&self) -> Result<GeminiClient, AppError> {
        self.ctx.create_llm("gemini-3-flash-preview").await
    }

    pub(super) async fn create_weak_llm(&self) -> Result<GeminiClient, AppError> {
        self.ctx.create_llm("gemini-2.5-flash").await
    }

    pub(super) async fn create_reasoning_llm(&self) -> Result<GeminiClient, AppError> {
        self.ctx.create_llm("gemini-3.1-pro-preview").await
    }
}

fn sanitize_research_result(value: &Value) -> Value {
    if let Some(obj) = value.as_object() {
        let mut out = serde_json::Map::new();
        for key in [
            "success",
            "truncated",
            "data_omitted",
            "saved_output_path",
            "hint",
            "structure_hint",
            "output_notice",
            "error",
            "total_lines",
            "matches",
            "result",
            "content",
        ] {
            if let Some(v) = obj.get(key) {
                out.insert(key.to_string(), v.clone());
            }
        }
        if out.is_empty() {
            serde_json::json!({"summary": "tool executed"})
        } else {
            Value::Object(out)
        }
    } else {
        value.clone()
    }
}

#[derive(Debug, Clone)]
struct ParsedSearchHit {
    path: String,
    line_number: Option<usize>,
    line_text: String,
}

#[derive(Debug, Clone, Default)]
struct FileCandidate {
    path: String,
    hit_count: usize,
    lines: Vec<usize>,
    sample: String,
    score: f32,
}

fn extract_search_hits(result: &Value) -> Vec<ParsedSearchHit> {
    let mut out = Vec::new();
    let Some(raw) = result.get("matches").and_then(|v| v.as_str()) else {
        return out;
    };

    for line in raw.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("match") {
            continue;
        }
        let Some(data) = v.get("data") else {
            continue;
        };
        let path = data
            .get("path")
            .and_then(|p| p.get("text"))
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string();
        if path.is_empty() || !is_high_signal_file(&path) {
            continue;
        }
        let line_number = data
            .get("line_number")
            .and_then(|n| n.as_u64())
            .map(|n| n as usize);
        let line_text = data
            .get("lines")
            .and_then(|l| l.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        out.push(ParsedSearchHit {
            path,
            line_number,
            line_text,
        });
    }

    out
}

fn is_high_signal_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    if lower.contains("/node_modules/")
        || lower.contains("/dist/")
        || lower.contains("/build/")
        || lower.contains(".min.")
        || lower.ends_with(".lock")
    {
        return false;
    }
    true
}

fn update_file_candidates(
    candidates: &mut HashMap<String, FileCandidate>,
    hits: &[ParsedSearchHit],
) {
    for hit in hits {
        let entry = candidates
            .entry(hit.path.clone())
            .or_insert_with(|| FileCandidate {
                path: hit.path.clone(),
                ..Default::default()
            });
        entry.hit_count += 1;
        if let Some(ln) = hit.line_number {
            entry.lines.push(ln);
        }
        if entry.sample.is_empty() && !hit.line_text.is_empty() {
            entry.sample = truncate_for_research(&hit.line_text, 180);
        }
        entry.score = entry.hit_count as f32 + (entry.lines.len() as f32 * 0.2);
    }
}

fn ranked_file_candidates(candidates: &HashMap<String, FileCandidate>) -> Vec<FileCandidate> {
    let mut list: Vec<FileCandidate> = candidates.values().cloned().collect();
    list.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    list
}

fn pick_best_read_target(
    requested_path: Option<&str>,
    candidates: &HashMap<String, FileCandidate>,
) -> Option<(String, Option<usize>)> {
    if let Some(path) = requested_path {
        if let Some(c) = candidates.get(path) {
            let ln = c.lines.first().copied();
            return Some((path.to_string(), ln));
        }
    }
    let ranked = ranked_file_candidates(candidates);
    ranked
        .first()
        .map(|c| (c.path.clone(), c.lines.first().copied()))
}

fn score_read_evidence(content: &str, query: &str) -> f32 {
    let q_terms: Vec<String> = query
        .split_whitespace()
        .map(|s| s.to_lowercase())
        .filter(|s| s.len() >= 3)
        .collect();
    if q_terms.is_empty() {
        return 0.5;
    }
    let lc = content.to_lowercase();
    let matched = q_terms.iter().filter(|t| lc.contains(t.as_str())).count();
    0.4 + (matched as f32 / q_terms.len() as f32) * 0.6
}

fn completion_allowed(
    output: &str,
    target_output: &str,
    read_evidence: &[dto::json::agent_executor::ContextChunkMatch],
) -> bool {
    if output.trim().len() < 40 {
        return false;
    }
    if read_evidence.is_empty() {
        return false;
    }
    let out = output.to_lowercase();
    let target_terms: Vec<String> = target_output
        .split_whitespace()
        .map(|s| {
            s.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|s| s.len() >= 4)
        .take(8)
        .collect();
    if target_terms.is_empty() {
        return true;
    }
    let overlap = target_terms
        .iter()
        .filter(|t| out.contains(t.as_str()))
        .count();
    overlap >= 2 || (overlap as f32 / target_terms.len() as f32) >= 0.35
}

fn build_recommended_files(
    candidates: &HashMap<String, FileCandidate>,
    evidence: &[dto::json::agent_executor::ContextChunkMatch],
) -> Vec<dto::json::agent_executor::RecommendedFile> {
    let mut ranked = ranked_file_candidates(candidates);
    ranked.truncate(8);
    let evidence_paths: HashSet<&str> = evidence.iter().map(|c| c.path.as_str()).collect();

    ranked
        .into_iter()
        .map(|c| dto::json::agent_executor::RecommendedFile {
            path: c.path.clone(),
            document_title: c.path.rsplit('/').next().unwrap_or("file").to_string(),
            relevance_score: c.score,
            reason: if evidence_paths.contains(c.path.as_str()) {
                format!(
                    "High-signal file with {} hits and validated read evidence",
                    c.hit_count
                )
            } else {
                format!("High-signal file with {} search hit(s)", c.hit_count)
            },
            sample_text: if c.sample.is_empty() {
                None
            } else {
                Some(c.sample.clone())
            },
        })
        .collect()
}

fn truncate_for_research(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut out = String::new();
    for ch in input.chars().take(max_chars) {
        out.push(ch);
    }
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::AgentExecutor;

    #[test]
    fn detects_internal_reasoning_dump_with_numbered_lines() {
        let text = "1. The user is asking about memory.\n2. I need to perform a universal search across all categories.\n3. The user is asking if I have any data in memory.";
        assert!(AgentExecutor::looks_like_internal_reasoning_dump(text));
    }

    #[test]
    fn does_not_flag_normal_user_facing_text() {
        let text = "I checked and I do not have stored memory records yet.";
        assert!(!AgentExecutor::looks_like_internal_reasoning_dump(text));
    }

    #[test]
    fn sanitizes_internal_reasoning_dump_to_fallback() {
        let text = "1. The user is asking about memory.\n2. I need to perform a universal search across all categories.";
        let sanitized = AgentExecutor::sanitize_user_facing_message(text, "Fallback");
        assert_eq!(sanitized, "Fallback");
    }

    #[test]
    fn decision_signature_similarity_detects_near_duplicates() {
        let a = "execute:read_file:inspect billing config and usage limits";
        let b = "execute:read_file:inspect usage limits in billing config";
        assert!(AgentExecutor::decision_signatures_similar(a, b));
    }

    #[test]
    fn decision_signature_similarity_rejects_different_step_kinds() {
        let a = "loadmemory:universal:billing";
        let b = "execute:read_file:billing";
        assert!(!AgentExecutor::decision_signatures_similar(a, b));
    }
}
