use super::core::{AgentExecutor, ResumeContext};

use crate::llm::{LlmRole, ResolvedLlm, UsageMetadata};

use commands::UpdateAgentThreadStateCommand;
use common::error::AppError;
use dto::json::agent_executor::ApprovalRequestData;
use dto::json::agent_executor::ConverseRequest;
use dto::json::StreamEvent;
use models::{
    AgentThreadStatus, ConversationContent, ConversationMessageType, RequestedToolApproval,
    RequestedToolApprovalState, ThreadExecutionState, ToolApprovalMode, ToolApprovalRequestState,
};
use std::collections::HashSet;

const LONG_THINK_INPUT_TOKEN_BUDGET: u32 = 2_000_000;
const LONG_THINK_OUTPUT_TOKEN_BUDGET: u32 = 300_000;
const LONG_THINK_CREDIT_WINDOW_MILLIS: i64 = 30 * 60 * 1000;

impl AgentExecutor {
    async fn cleanup_filesystem(&mut self) {
        if let Err(e) = self.filesystem.cleanup().await {
            let _ = e;
        }
    }

    async fn mark_thread_running(
        &self,
        execution_state: Option<ThreadExecutionState>,
    ) -> Result<(), AppError> {
        let mut command =
            UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
                .with_status(AgentThreadStatus::Running);
        if let Some(execution_state) = execution_state {
            command = command.with_execution_state(execution_state);
        }

        command
            .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
            .await
    }

    async fn load_context_for_execution_trigger(
        &mut self,
        trigger_conversation: &models::ConversationRecord,
    ) -> Result<(), AppError> {
        if let Err(error) = self
            .compact_history_before_execution_if_needed(trigger_conversation)
            .await
        {
            UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
                .with_status(AgentThreadStatus::Idle)
                .with_execution_state(self.build_execution_state_snapshot(None))
                .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
                .await?;
            return Err(error);
        }

        let context = self.get_immediate_context().await?;
        self.conversations = context.conversations;
        if !self
            .conversations
            .iter()
            .any(|conversation| conversation.id == trigger_conversation.id)
        {
            self.conversations.push(trigger_conversation.clone());
        }
        self.memories = context.memories;

        Ok(())
    }

    pub async fn resume_execution(
        &mut self,
        resume_context: ResumeContext,
    ) -> Result<(), AppError> {
        let result = self.resume_execution_inner(resume_context).await;
        self.cleanup_filesystem().await;
        result
    }

    async fn resume_execution_inner(
        &mut self,
        resume_context: ResumeContext,
    ) -> Result<(), AppError> {
        let thread_id = self.ctx.thread_id;
        let deployment_id = self.ctx.agent.deployment_id;
        let app_state = self.ctx.app_state.clone();

        let immediate_context = self.get_immediate_context().await?;
        self.conversations = immediate_context.conversations;
        self.memories = immediate_context.memories;

        match resume_context {
            ResumeContext::ApprovalResponse(approvals) => {
                self.apply_tool_approval_response(&approvals).await?;
            }
        }

        let _ = (thread_id, deployment_id, app_state);
        self.mark_thread_running(Some(self.build_execution_state_snapshot(None)))
            .await?;

        self.repl().await
    }

    pub async fn execute_with_conversation_id(
        &mut self,
        conversation_id: i64,
    ) -> Result<(), AppError> {
        let request = ConverseRequest { conversation_id };
        self.run(request).await
    }

    pub async fn run(&mut self, request: ConverseRequest) -> Result<(), AppError> {
        let result = self.run_inner(request).await;
        self.cleanup_filesystem().await;
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

        self.mark_thread_running(None).await?;

        let _ = self
            .channel
            .send(StreamEvent::ConversationMessage(conversation.clone()))
            .await;

        self.load_context_for_execution_trigger(&conversation)
            .await?;

        self.repl().await?;
        Ok(())
    }

    pub async fn execute_with_thread_event(
        &mut self,
        thread_event: models::ThreadEvent,
    ) -> Result<(), AppError> {
        let result = self.run_thread_event_inner(thread_event).await;
        self.cleanup_filesystem().await;
        result
    }

    async fn run_thread_event_inner(
        &mut self,
        thread_event: models::ThreadEvent,
    ) -> Result<(), AppError> {
        self.active_thread_event = Some(thread_event.clone());

        let thread_event_message = self.build_thread_event_message(&thread_event).await?;
        let conversation = if !Self::should_create_worker_event_message(&thread_event) {
            if let Some(conversation_id) = thread_event.caused_by_conversation_id {
                let conversation = queries::GetConversationByIdQuery::new(conversation_id)
                    .execute_with_db(self.ctx.app_state.db_router.writer())
                    .await?;
                let _ = self
                    .channel
                    .send(StreamEvent::ConversationMessage(conversation.clone()))
                    .await;
                conversation
            } else {
                self.store_user_message(thread_event_message.clone(), None)
                    .await?
            }
        } else {
            self.store_user_message(thread_event_message.clone(), None)
                .await?
        };

        self.user_request = match &conversation.content {
            models::ConversationContent::UserMessage { message, .. } => message.clone(),
            _ => thread_event_message,
        };

        self.mark_thread_running(None).await?;

        self.load_context_for_execution_trigger(&conversation)
            .await?;

        if matches!(
            thread_event.event_type.as_str(),
            "task_routing" | "assignment_outcome_review"
        ) && self.effective_is_coordinator_thread()
        {
            self.refresh_project_task_board_items().await?;
        }

        let result = self.repl().await;
        self.active_thread_event = None;
        result
    }

    pub(crate) fn build_execution_state_snapshot(
        &self,
        pending_approval_request: Option<ToolApprovalRequestState>,
    ) -> ThreadExecutionState {
        ThreadExecutionState {
            long_think_credit_snapshot: self.long_think_credit_snapshot.clone(),
            loaded_external_tool_ids: self.loaded_external_tool_ids.clone(),
            prompt_caches: models::PromptCacheRegistry {
                step_decision: self.next_step_decision_cache_state.clone(),
                action_loop: None,
            },
            pending_approval_request,
            active_startaction_directive: self
                .active_startaction_directive
                .as_ref()
                .and_then(|directive| serde_json::to_value(directive).ok()),
            active_tool_call_brief: self
                .active_tool_call_brief
                .as_ref()
                .and_then(|brief| serde_json::to_value(brief).ok()),
            assignment_outcome_override: None,
            task_journal_start_hash: self.task_journal_start_hash.clone(),
            conversation_compaction_state: self.conversation_compaction_state.clone(),
        }
    }

    pub(crate) fn refresh_long_think_credits(&mut self) {
        let now = chrono::Utc::now();
        let elapsed_ms = (now - self.long_think_credit_snapshot.snapshot_at).num_milliseconds();
        if elapsed_ms <= 0 {
            return;
        }

        let input_refill = ((elapsed_ms as i128 * LONG_THINK_INPUT_TOKEN_BUDGET as i128)
            / LONG_THINK_CREDIT_WINDOW_MILLIS as i128) as u32;
        let output_refill = ((elapsed_ms as i128 * LONG_THINK_OUTPUT_TOKEN_BUDGET as i128)
            / LONG_THINK_CREDIT_WINDOW_MILLIS as i128) as u32;

        self.long_think_credit_snapshot.input_tokens_available = self
            .long_think_credit_snapshot
            .input_tokens_available
            .saturating_add(input_refill)
            .min(LONG_THINK_INPUT_TOKEN_BUDGET);
        self.long_think_credit_snapshot.output_tokens_available = self
            .long_think_credit_snapshot
            .output_tokens_available
            .saturating_add(output_refill)
            .min(LONG_THINK_OUTPUT_TOKEN_BUDGET);
        self.long_think_credit_snapshot.snapshot_at = now;
    }

    pub(crate) fn long_think_credits_available(&self) -> bool {
        self.long_think_credit_snapshot.input_tokens_available > 0
            && self.long_think_credit_snapshot.output_tokens_available > 0
    }

    pub(crate) fn consume_long_think_credits(&mut self, usage: Option<&UsageMetadata>) {
        self.refresh_long_think_credits();
        let now = chrono::Utc::now();

        if let Some(usage) = usage {
            let output_tokens = usage
                .candidates_token_count
                .saturating_add(usage.thoughts_token_count.unwrap_or(0));

            self.long_think_credit_snapshot.input_tokens_available = self
                .long_think_credit_snapshot
                .input_tokens_available
                .saturating_sub(usage.prompt_token_count);
            self.long_think_credit_snapshot.output_tokens_available = self
                .long_think_credit_snapshot
                .output_tokens_available
                .saturating_sub(output_tokens);
        }

        self.long_think_credit_snapshot.snapshot_at = now;
        self.long_think_credit_snapshot.last_used_at = Some(now);
    }

    pub(crate) fn record_llm_usage_for_compaction(&mut self, usage: Option<&UsageMetadata>) {
        let Some(usage) = usage else {
            return;
        };

        self.conversation_compaction_state.last_prompt_token_count = usage.prompt_token_count;
        self.conversation_compaction_state.last_total_token_count = usage.total_token_count;
        self.conversation_compaction_state
            .max_prompt_token_count_seen = self
            .conversation_compaction_state
            .max_prompt_token_count_seen
            .max(usage.prompt_token_count);
    }

    pub(crate) fn long_think_nudge(&self) -> Option<String> {
        if !self.long_think_mode_active {
            return None;
        }

        Some(format!(
            "High-thinking mode is active for this next-step decision and this is accruing rolling credit usage. Remaining credits right now: input {} / {}, output {} / {}. Use the extra thinking to get unstuck, then step back down by choosing a concrete next step instead of requesting high-thinking mode again.",
            self.long_think_credit_snapshot.input_tokens_available,
            LONG_THINK_INPUT_TOKEN_BUDGET,
            self.long_think_credit_snapshot.output_tokens_available,
            LONG_THINK_OUTPUT_TOKEN_BUDGET
        ))
    }

    pub(crate) async fn request_user_approval(
        &mut self,
        approval_request: ApprovalRequestData,
    ) -> Result<(), AppError> {
        let effective_approved_tool_ids = self.effective_approved_tool_ids().await?;
        let approval_description = if approval_request.description.trim().is_empty() {
            format!(
                "Approval required to run: {}.",
                approval_request.tool_names.join(", ")
            )
        } else {
            approval_request.description.trim().to_string()
        };
        let mut seen = HashSet::new();
        let requested_tools = approval_request
            .tool_names
            .iter()
            .filter_map(|tool_name| {
                if !seen.insert(tool_name.clone()) {
                    return None;
                }
                self.ctx
                    .agent
                    .tools
                    .iter()
                    .find(|tool| tool.name == *tool_name)
                    .filter(|tool| tool.requires_user_approval)
                    .filter(|tool| !effective_approved_tool_ids.contains(&tool.id))
                    .map(|tool| RequestedToolApprovalState {
                        tool_id: tool.id,
                        tool_name: tool.name.clone(),
                        tool_description: tool.description.clone(),
                    })
            })
            .collect::<Vec<_>>();

        if requested_tools.is_empty() {
            return Err(AppError::BadRequest(
                "Approval request must include at least one approval-gated tool without an active grant"
                    .to_string(),
            ));
        }

        let request_message_id = self.ctx.app_state.sf.next_id()? as i64;
        let approval_conversation = self
            .create_conversation_with_id(
                request_message_id,
                ConversationContent::ApprovalRequest {
                    description: approval_description.clone(),
                    tools: requested_tools
                        .iter()
                        .map(|tool| RequestedToolApproval {
                            tool_id: tool.tool_id,
                            tool_name: tool.tool_name.clone(),
                            tool_description: tool.tool_description.clone(),
                        })
                        .collect(),
                },
                ConversationMessageType::ApprovalRequest,
                None,
            )
            .await?;

        let pending_approval_request = ToolApprovalRequestState {
            request_message_id: Some(request_message_id.to_string()),
            description: approval_description,
            tools: requested_tools,
        };

        UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
            .with_execution_state(
                self.build_execution_state_snapshot(Some(pending_approval_request)),
            )
            .with_status(AgentThreadStatus::WaitingForInput)
            .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
            .await?;

        self.conversations.push(approval_conversation.clone());
        let _ = self
            .channel
            .send(StreamEvent::ConversationMessage(approval_conversation))
            .await;

        Ok(())
    }

    async fn apply_tool_approval_response(
        &mut self,
        approvals: &[dto::json::deployment::ToolApprovalSelection],
    ) -> Result<(), AppError> {
        let mut seen = HashSet::new();
        for approval in approvals {
            if approval.tool_name.trim().is_empty() {
                return Err(AppError::BadRequest(
                    "Approval response tool names must be non-empty".to_string(),
                ));
            }
            if !seen.insert(approval.tool_name.clone()) {
                return Err(AppError::BadRequest(format!(
                    "Approval response contains duplicate tool '{}'",
                    approval.tool_name
                )));
            }
            let Some(tool) = self
                .ctx
                .agent
                .tools
                .iter()
                .find(|tool| tool.name == approval.tool_name)
            else {
                return Err(AppError::BadRequest(format!(
                    "Approval response references unknown tool '{}'",
                    approval.tool_name
                )));
            };

            if matches!(approval.mode, ToolApprovalMode::AllowAlways) {
                self.approved_always_tool_ids.insert(tool.id);
            }
        }

        Ok(())
    }

    pub(crate) async fn effective_approved_tool_ids(&self) -> Result<HashSet<i64>, AppError> {
        let mut approved_tool_ids = self.approved_always_tool_ids.clone();

        let active_approvals = queries::ListActiveApprovalGrantsForThreadQuery::new(
            self.ctx.agent.deployment_id,
            self.ctx.thread_id,
        )
        .execute_with_db(self.ctx.app_state.db_router.writer())
        .await?;

        for approval in active_approvals {
            approved_tool_ids.insert(approval.tool_id);
        }

        Ok(approved_tool_ids)
    }

    pub(crate) async fn create_strong_llm(&self) -> Result<ResolvedLlm, AppError> {
        self.ctx.create_llm(LlmRole::Strong).await
    }

    pub(crate) async fn create_weak_llm(&self) -> Result<ResolvedLlm, AppError> {
        self.ctx.create_llm(LlmRole::Weak).await
    }

    pub(crate) fn next_step_decision_cache_key(&self, model_name: &str) -> String {
        format!("{}:next-step-decision:{model_name}", self.ctx.thread_id)
    }

    pub(crate) async fn persist_next_step_decision_cache_state(
        &mut self,
        cache_state: Option<models::PromptCacheState>,
    ) -> Result<(), AppError> {
        if self.next_step_decision_cache_state == cache_state {
            return Ok(());
        }

        self.next_step_decision_cache_state = cache_state;
        UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
            .with_execution_state(self.build_execution_state_snapshot(None))
            .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
            .await?;
        self.ctx.invalidate_cache();
        Ok(())
    }
}
