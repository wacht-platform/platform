use super::core::{AgentExecutor, ResumeContext};

use crate::executor::context::memory_context::load_immediate_context;
use crate::llm::{LlmRole, ResolvedLlm, UsageMetadata};

use commands::UpdateAgentThreadStateCommand;
use common::error::AppError;
use dto::json::agent_executor::ApprovalRequestData;
use dto::json::agent_executor::ConverseRequest;
use dto::json::StreamEvent;
use models::{
    AgentThreadStatus, ConversationContent, ConversationMessageType, PromptCacheState,
    RequestedToolApproval, RequestedToolApprovalState, ThreadExecutionState, ToolApprovalMode,
    ToolApprovalRequestState,
};
use redis::AsyncCommands;
use std::collections::HashSet;

impl AgentExecutor {
    async fn take_or_load_immediate_context(
        &mut self,
    ) -> Result<models::ImmediateContext, AppError> {
        match self.preloaded_immediate_context.take() {
            Some(c) => Ok(c),
            None => load_immediate_context(&self.ctx, self.current_board_item_id()).await,
        }
    }

    async fn mark_thread_running(
        &self,
        execution_state: Option<ThreadExecutionState>,
    ) -> Result<(), AppError> {
        let mut command = self.apply_thread_status(
            UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id),
            AgentThreadStatus::Running,
        );
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
        let compact_ran = match self
            .compact_history_before_execution_if_needed(trigger_conversation)
            .await
        {
            Ok(ran) => ran,
            Err(error) => {
                self.apply_thread_status(
                    UpdateAgentThreadStateCommand::new(
                        self.ctx.thread_id,
                        self.ctx.agent.deployment_id,
                    ),
                    AgentThreadStatus::Idle,
                )
                .with_execution_state(self.build_execution_state_snapshot(None))
                .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
                .await?;
                return Err(error);
            }
        };

        if compact_ran {
            self.preloaded_immediate_context = None;
        }
        let (context, caches) = if self.preloaded_immediate_context.is_some() {
            let context = self.take_or_load_immediate_context().await?;
            let caches = self.fetch_all_prompt_caches().await?;
            (context, caches)
        } else {
            tokio::try_join!(
                load_immediate_context(&self.ctx, self.current_board_item_id()),
                self.fetch_all_prompt_caches(),
            )?
        };
        let (thread_mode, task_graph, board_context, tool_context) = caches;
        self.thread_mode_cache = Some(thread_mode);
        self.task_graph_snapshot = Some(task_graph);
        self.board_context_cache = Some(board_context);
        self.tool_context_cache = Some(tool_context);
        self.conversations = context.conversations;
        if !self
            .conversations
            .iter()
            .any(|conversation| conversation.id == trigger_conversation.id)
        {
            self.conversations.push(trigger_conversation.clone());
        }
        self.memories = context.memories;
        self.routing_events = context.routing_events;
        self.task_thread_meta = context.task_thread_meta;

        Ok(())
    }

    pub async fn resume_execution(
        &mut self,
        resume_context: ResumeContext,
    ) -> Result<(), AppError> {
        let result = self.resume_execution_inner(resume_context).await;
        result
    }

    async fn resume_execution_inner(
        &mut self,
        resume_context: ResumeContext,
    ) -> Result<(), AppError> {
        let immediate_context = self.take_or_load_immediate_context().await?;
        self.conversations = immediate_context.conversations;
        self.memories = immediate_context.memories;
        self.routing_events = immediate_context.routing_events;
        self.task_thread_meta = immediate_context.task_thread_meta;

        match resume_context {
            ResumeContext::ApprovalResponse(approvals) => {
                self.apply_tool_approval_response(&approvals).await?;
            }
        }

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
        result
    }

    async fn run_inner(&mut self, request: ConverseRequest) -> Result<(), AppError> {
        let preloaded_trigger = self.preloaded_immediate_context.as_ref().and_then(|ctx| {
            ctx.conversations
                .iter()
                .find(|c| c.id == request.conversation_id)
                .cloned()
        });
        let conversation = match preloaded_trigger {
            Some(c) => c,
            None => {
                queries::GetConversationByIdQuery::new(request.conversation_id)
                    .execute_with_db(self.ctx.app_state.db_router.writer())
                    .await?
            }
        };

        let user_message = match &conversation.content {
            models::ConversationContent::UserMessage { message, .. } => message.clone(),
            models::ConversationContent::ClarificationResponse { .. }
            | models::ConversationContent::ApprovalResponse { .. } => String::new(),
            _ => {
                return Err(AppError::BadRequest(
                    "Conversation must be a user message".to_string(),
                ))
            }
        };

        self.user_request = user_message;

        let _ = self
            .channel
            .send(StreamEvent::ConversationMessage(conversation.clone()))
            .await;

        self.load_context_for_execution_trigger(&conversation)
            .await?;

        self.repl().await?;
        Ok(())
    }

    async fn resolve_task_key_for_board_item(&self, board_item_id: i64) -> Option<String> {
        if board_item_id == 0 {
            return None;
        }
        queries::GetProjectTaskBoardItemByIdQuery::new(board_item_id)
            .execute_with_db(
                self.ctx
                    .app_state
                    .db_router
                    .reader(common::ReadConsistency::Strong),
            )
            .await
            .ok()
            .flatten()
            .map(|item| item.task_key)
    }

    pub async fn execute_with_thread_event(
        &mut self,
        thread_event: models::ThreadEvent,
    ) -> Result<(), AppError> {
        self.active_thread_event = Some(thread_event.clone());
        self.tool_executor
            .set_active_board_item_id(thread_event.board_item_id);

        // build_thread_event_message renders the full brief (and runs its
        // side effects — journal hash init, schedule carryover). For
        // trigger events we DON'T persist the brief as a fat conversation
        // row; we persist a thin marker and stash the brief in
        // current_trigger_brief so the prompt builder can rehydrate the
        // latest marker. Rerunning the agent on a future iteration reads
        // canonical state (DB + filesystem) fresh — old markers carry no
        // stale snapshot.
        let thread_event_message = self.build_thread_event_message(&thread_event).await?;

        let conversation = match thread_event.event_type.as_str() {
            models::thread_event::event_type::THREAD_SUBSCRIPTION_DELIVERY => {
                self.store_subscription_delivery_message(thread_event_message.clone())
                    .await?
            }
            models::thread_event::event_type::ASSIGNMENT_EXECUTION => {
                let payload = thread_event.assignment_execution_payload();
                let assignment_id = payload
                    .as_ref()
                    .map(|p| p.assignment_id)
                    .unwrap_or_default();
                let board_item_id = thread_event.board_item_id.unwrap_or_default();
                let task_key = self
                    .resolve_task_key_for_board_item(board_item_id)
                    .await
                    .unwrap_or_default();
                self.current_trigger_brief = Some(thread_event_message.clone());
                self.store_assignment_execution_trigger(
                    assignment_id,
                    board_item_id,
                    task_key,
                    None,
                )
                .await?
            }
            models::thread_event::event_type::TASK_ROUTING => {
                let routing_reason = thread_event
                    .task_routing_payload()
                    .and_then(|p| p.routing_reason);
                let board_item_id = thread_event.board_item_id.unwrap_or_default();
                let task_key = self
                    .resolve_task_key_for_board_item(board_item_id)
                    .await
                    .unwrap_or_default();
                self.current_trigger_brief = Some(thread_event_message.clone());
                let conv = self
                    .store_task_routing_trigger(board_item_id, task_key, routing_reason)
                    .await?;
                if board_item_id != 0 && self.effective_is_coordinator_thread() {
                    self.ensure_coordinator_assignment(board_item_id).await?;
                }
                conv
            }
            _ => {
                self.store_user_message(thread_event_message.clone(), None)
                    .await?
            }
        };

        self.user_request = match &conversation.content {
            models::ConversationContent::UserMessage { message, .. } => message.clone(),
            models::ConversationContent::TaskSubscriptionDelivery { summary } => summary.clone(),
            _ => thread_event_message,
        };

        self.load_context_for_execution_trigger(&conversation)
            .await?;

        if thread_event.event_type.as_str() == "task_routing"
            && self.effective_is_coordinator_thread()
        {
            self.refresh_project_task_board_items().await?;
        }

        let result = self.repl().await;
        self.finalize_coordinator_assignment().await;
        self.active_thread_event = None;
        self.current_trigger_brief = None;
        self.tool_executor.set_active_board_item_id(None);
        result
    }

    /// Look up an existing in-flight coordinator-role assignment for this
    /// board item / thread, or create one. Stash (assignment_id,
    /// board_item_id) so [`finalize_coordinator_assignment`] can close it
    /// at end-of-iteration.
    async fn ensure_coordinator_assignment(&mut self, board_item_id: i64) -> Result<(), AppError> {
        let assignment_id = commands::project_task_board::EnsureCoordinatorAssignmentCommand::new(
            board_item_id,
            self.ctx.thread_id,
        )
        .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
        .await?;
        self.coordinator_assignment = Some((assignment_id, board_item_id));
        Ok(())
    }

    /// Complete the coordinator-owned assignment if one was created for
    /// this iteration. Errors are logged but not propagated — the
    /// iteration's actual result (from `repl`) must remain the source of
    /// truth for the caller.
    async fn finalize_coordinator_assignment(&mut self) {
        let Some((assignment_id, board_item_id)) = self.coordinator_assignment.take() else {
            return;
        };
        let cmd = commands::project_task_board::MarkCoordinatorAssignmentCompletedCommand::new(
            assignment_id,
            self.ctx.thread_id,
            board_item_id,
        );
        if let Err(e) = cmd
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
        {
            tracing::warn!(
                thread_id = self.ctx.thread_id,
                assignment_id,
                board_item_id,
                error = ?e,
                "failed to complete coordinator-role assignment at end of iteration"
            );
        }
    }

    pub(crate) fn build_execution_state_snapshot(
        &self,
        pending_approval_request: Option<ToolApprovalRequestState>,
    ) -> ThreadExecutionState {
        const MAX_CACHED_VIRTUAL_TOOLS: usize = 200;
        let loaded_set: std::collections::HashSet<i64> =
            self.loaded_external_tool_ids.iter().copied().collect();
        let mut virtual_tool_cache_snapshot: Vec<models::AiTool> = self
            .loaded_external_tool_ids
            .iter()
            .filter_map(|id| self.virtual_tool_cache.get(id).cloned())
            .collect();
        for (id, tool) in &self.virtual_tool_cache {
            if loaded_set.contains(id) {
                continue;
            }
            if virtual_tool_cache_snapshot.len() >= MAX_CACHED_VIRTUAL_TOOLS {
                break;
            }
            virtual_tool_cache_snapshot.push(tool.clone());
        }
        ThreadExecutionState {
            loaded_external_tool_ids: self.loaded_external_tool_ids.clone(),
            virtual_tool_cache_snapshot,
            pending_approval_request,
            assignment_outcome_override: None,
            task_journal_start_hash: self.task_journal_start_hash.clone(),
            conversation_compaction_state: self.conversation_compaction_state.clone(),
            pending_question: self.pending_question.clone(),
        }
    }

    fn prompt_cache_redis_key(&self, cache_key: &str) -> String {
        format!("agent:prompt_cache:{}:{cache_key}", self.ctx.thread_id)
    }

    pub(crate) async fn build_prompt_cache_request(
        &self,
        live_tail_count: usize,
    ) -> Option<crate::llm::PromptCacheRequest> {
        // `AGENT_ENGINE_CACHE_MODE` toggles the prompt caching strategy:
        //   - "explicit" (default): explicit Gemini `cachedContents` lifecycle
        //     managed by us (cache.rs + Redis state). Guaranteed reuse, write
        //     cost on first call, fixed TTL.
        //   - "implicit": skip our cache plan entirely. Gemini 2.5+ models
        //     implicitly cache identical prefixes server-side at no write cost.
        //     We just need to keep the prefix stable (which the prompt builder
        //     already does after the live_tail_count / stable_context fixes).
        //   - "off": disable both. Useful for A/B baseline.
        let mode = std::env::var("AGENT_ENGINE_CACHE_MODE")
            .unwrap_or_else(|_| "explicit".to_string())
            .to_ascii_lowercase();
        if mode == "implicit" || mode == "off" {
            return None;
        }

        let cache_key = if let Some(event) = self.active_thread_event.as_ref() {
            if let Some(payload) = event.assignment_execution_payload() {
                format!("executor_assignment_{}", payload.assignment_id)
            } else if let Some(board_item_id) = event.board_item_id {
                format!("coordinator_board_{board_item_id}")
            } else {
                "thread_default".to_string()
            }
        } else if self.is_conversation_thread {
            "conversation".to_string()
        } else {
            return None;
        };

        let prior_state = self.read_prompt_cache_state(&cache_key).await;
        Some(crate::llm::PromptCacheRequest {
            cache_key,
            ttl_secs: 300,
            live_tail_count,
            prior_state,
            reuse_only: false,
        })
    }

    async fn read_prompt_cache_state(&self, cache_key: &str) -> Option<PromptCacheState> {
        let mut conn = self
            .ctx
            .app_state
            .redis_client
            .get_multiplexed_async_connection()
            .await
            .ok()?;
        let raw: String = conn
            .get(self.prompt_cache_redis_key(cache_key))
            .await
            .ok()?;
        let state: PromptCacheState = serde_json::from_str(&raw).ok()?;
        if state.is_expired(chrono::Utc::now()) {
            return None;
        }
        Some(state)
    }

    pub(crate) async fn write_prompt_cache_state(&self, state: &PromptCacheState) {
        let Ok(json) = serde_json::to_string(state) else {
            return;
        };
        let Ok(mut conn) = self
            .ctx
            .app_state
            .redis_client
            .get_multiplexed_async_connection()
            .await
        else {
            return;
        };
        let ttl = (state.expire_at - chrono::Utc::now()).num_seconds().max(1) as u64;
        let _: Result<(), _> = conn
            .set_ex(self.prompt_cache_redis_key(&state.cache_key), json, ttl)
            .await;
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
                if !matches!(
                    crate::tools::approval::resolve_approval_action(&self.ctx.agent, tool_name),
                    models::ApprovalAction::Review
                ) {
                    return None;
                }
                self.ctx
                    .agent
                    .tools
                    .iter()
                    .find(|tool| tool.name == *tool_name)
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

        if let Some(board_item_id) = self.current_board_item_id() {
            commands::SetBoardItemPendingApprovalCommand {
                board_item_id,
                pending_approval: Some(pending_approval_request.clone()),
            }
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;
        }

        self.apply_thread_status(
            UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
                .with_execution_state(
                    self.build_execution_state_snapshot(Some(pending_approval_request)),
                ),
            AgentThreadStatus::WaitingForInput,
        )
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

        if let Some(board_item_id) = self.current_board_item_id() {
            commands::SetBoardItemPendingApprovalCommand {
                board_item_id,
                pending_approval: None,
            }
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;
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
}
