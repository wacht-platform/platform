use super::core::AgentExecutor;
use super::STEP_DECISION_CACHE_TTL_SECS;
use crate::executor::runtime::step_control::{
    validate_next_step_decision, NextStepDecisionValidationContext,
};

use common::error::AppError;
use dto::json::agent_executor::NextStepDecision;
use dto::json::{
    KnowledgeBasePromptItem, LlmHistoryEntry, LlmHistoryPart, NextStepDecisionContext,
    NextStepDecisionConversationContext, NextStepDecisionPromptEnvelope,
    NextStepDecisionResourceContext, NextStepDecisionRuntimeContext, NextStepDecisionTaskContext,
    NextStepDecisionThreadContext, SkillPromptItem, ToolPromptItem,
};
use std::collections::HashSet;

use crate::filesystem::mounts::heartbeat_deployment_root;
use crate::llm::{
    PromptCacheRequest, SemanticLlmMessage, SemanticLlmPromptConfig, SemanticLlmRequest,
};
use crate::template::{
    render_prompt_text, render_template_json, render_template_only, AgentTemplates,
};
use queries::GetProjectTaskBoardItemAssignmentByIdQuery;
use serde_json::json;
const STEER_VISIBILITY_NUDGE_WINDOW: usize = 4;

struct ThreadModeContext {
    exec_context: models::AgentThreadState,
    is_coordinator_thread: bool,
    allows_user_interaction: bool,
}

struct ConversationPromptContext {
    conversation_history_prefix: Vec<LlmHistoryEntry>,
    current_request_entry: LlmHistoryEntry,
}

struct BoardPromptContext {
    active_assignment: Option<dto::json::ProjectTaskBoardAssignmentPromptItem>,
    active_board_item: Option<dto::json::ProjectTaskBoardPromptItem>,
    active_board_item_assignments: Vec<dto::json::ProjectTaskBoardAssignmentPromptItem>,
    recent_assignment_history: Vec<dto::json::ProjectTaskBoardAssignmentPromptItem>,
    active_board_item_events: Vec<dto::json::ProjectTaskBoardItemEventPromptItem>,
    task_journal_tail: Option<String>,
    thread_assignment_queue: Vec<dto::json::ProjectTaskBoardAssignmentPromptItem>,
    scoped_project_task_board_items: Vec<dto::json::ProjectTaskBoardPromptItem>,
}

struct ToolPromptContext {
    tool_prompt_items: Vec<ToolPromptItem>,
    knowledge_base_prompt_items: Vec<KnowledgeBasePromptItem>,
    system_skill_prompt_items: Vec<SkillPromptItem>,
    agent_skill_prompt_items: Vec<SkillPromptItem>,
    available_sub_agents: Vec<dto::json::SubAgentPromptInfo>,
    discoverable_external_tool_names: Vec<String>,
    loaded_external_tool_names: Vec<String>,
}

impl AgentExecutor {
    pub(super) fn latest_startaction_directive(
        &self,
    ) -> Option<dto::json::agent_executor::StartActionDirective> {
        self.active_startaction_directive.clone()
    }

    fn next_step_decision_metadata(decision: &NextStepDecision) -> Option<serde_json::Value> {
        if let Some(directive) = decision.startaction_directive.as_ref() {
            return Some(json!({ "startaction_directive": directive }));
        }
        if let Some(directive) = decision.continueaction_directive.as_ref() {
            return Some(json!({ "continueaction_directive": directive }));
        }
        None
    }

    fn steer_visibility_nudge(&self, allows_user_interaction: bool) -> Option<String> {
        if !allows_user_interaction {
            return None;
        }

        let mut recent_visible_messages = Vec::new();
        for conv in self.conversations.iter().rev() {
            match conv.message_type {
                models::ConversationMessageType::UserMessage => {
                    if recent_visible_messages.is_empty() {
                        return None;
                    }
                    break;
                }
                models::ConversationMessageType::Steer
                | models::ConversationMessageType::ApprovalRequest
                | models::ConversationMessageType::ApprovalResponse
                | models::ConversationMessageType::ExecutionSummary => {
                    recent_visible_messages.push(conv);
                    if recent_visible_messages.len() >= STEER_VISIBILITY_NUDGE_WINDOW {
                        break;
                    }
                }
                _ => {}
            }
        }

        if recent_visible_messages.len() < STEER_VISIBILITY_NUDGE_WINDOW {
            return None;
        }

        if recent_visible_messages
            .iter()
            .any(|conv| matches!(conv.message_type, models::ConversationMessageType::Steer))
        {
            return None;
        }

        Some(
            "No visible steer was sent in the last 4 conversation-visible messages. Before continuing another non-trivial action or multi-step execution run, strongly prefer one short steer with further_actions_required=true that states the exact direction you are taking right now. Do this unless the next step is a tiny immediate read/search/list action."
                .to_string(),
        )
    }

    pub(super) async fn decide_next_step(&mut self) -> Result<NextStepDecision, AppError> {
        if let Err(_error) = heartbeat_deployment_root(self.ctx.agent.deployment_id).await {}

        self.generate_next_step_decision().await
    }

    pub(crate) async fn build_next_step_decision_prompt_context_json(
        &mut self,
    ) -> Result<serde_json::Value, AppError> {
        let thread_mode = self.load_thread_mode_context().await?;
        let task_graph = self.ensure_task_graph_snapshot().await?;
        let task_graph_view = Self::render_task_graph_view(&task_graph);
        let board_context = self
            .load_board_prompt_context(thread_mode.is_coordinator_thread)
            .await?;
        let conversation_context = self.build_conversation_prompt_context().await;
        let tool_context = self
            .load_tool_prompt_context(thread_mode.is_coordinator_thread)
            .await?;
        let context = NextStepDecisionContext {
            runtime: NextStepDecisionRuntimeContext {
                current_datetime_utc: chrono::Utc::now()
                    .format("%Y-%m-%d %H:%M:%S UTC")
                    .to_string(),
                iteration_info: dto::json::IterationInfo {
                    current_iteration: self.current_iteration.max(1),
                    max_iterations: 50,
                },
                long_think_mode_active: self.long_think_mode_active,
                long_think_input_tokens_available: self
                    .long_think_credit_snapshot
                    .input_tokens_available,
                long_think_output_tokens_available: self
                    .long_think_credit_snapshot
                    .output_tokens_available,
                long_think_input_token_budget: 2_000_000,
                long_think_output_token_budget: 300_000,
                long_think_window_minutes: 30,
                long_think_nudge: self.long_think_nudge(),
                steer_visibility_nudge: self
                    .steer_visibility_nudge(thread_mode.allows_user_interaction),
            },
            conversation: NextStepDecisionConversationContext {
                user_request: self.user_request.clone(),
                triggering_event: self
                    .active_thread_event
                    .as_ref()
                    .map(Self::thread_event_prompt_item),
                input_safety_signals: self.derive_input_safety_signals(),
            },
            thread: NextStepDecisionThreadContext {
                id: self.ctx.thread_id,
                title: thread_mode.exec_context.title.clone(),
                purpose: if thread_mode.is_coordinator_thread {
                    models::agent_thread::purpose::COORDINATOR.to_string()
                } else {
                    thread_mode.exec_context.thread_purpose.clone()
                },
                responsibility: thread_mode.exec_context.responsibility.clone(),
            },
            resources: NextStepDecisionResourceContext {
                available_tools: tool_context.tool_prompt_items,
                available_knowledge_bases: tool_context.knowledge_base_prompt_items,
                available_system_skills: tool_context.system_skill_prompt_items,
                available_agent_skills: tool_context.agent_skill_prompt_items,
                available_sub_agents: tool_context.available_sub_agents,
            },
            task: NextStepDecisionTaskContext {
                project_task_board_items: board_context.scoped_project_task_board_items,
                active_board_item: board_context.active_board_item,
                active_assignment: board_context.active_assignment,
                active_board_item_assignments: board_context.active_board_item_assignments,
                recent_assignment_history: board_context.recent_assignment_history,
                active_board_item_events: board_context.active_board_item_events,
                task_journal_tail: board_context.task_journal_tail,
                thread_assignment_queue: board_context.thread_assignment_queue,
                task_graph_view: Some(task_graph_view),
            },
        };

        let mut prompt_context = NextStepDecisionPromptEnvelope {
            base: context,
            agent_name: self.ctx.agent.name.clone(),
            agent_description: self.ctx.agent.description.clone(),
            conversation_history_prefix: conversation_context.conversation_history_prefix.clone(),
            current_request_entry: conversation_context.current_request_entry,
            discoverable_external_tool_names: tool_context.discoverable_external_tool_names,
            loaded_external_tool_names: tool_context.loaded_external_tool_names,
            custom_system_instructions: self
                .system_instructions
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            live_context_message: None,
        };

        let prompt_context_value = serde_json::to_value(&prompt_context)?;

        let live_context_message = render_template_only(
            AgentTemplates::STEP_DECISION_LIVE_CONTEXT,
            &prompt_context_value,
        )
        .map_err(|e| {
            AppError::Internal(format!(
                "Failed to render next-step decision live context template: {e}"
            ))
        })?;

        prompt_context.live_context_message = Some(live_context_message);

        Ok(serde_json::to_value(&prompt_context)?)
    }

    pub(crate) fn build_next_step_decision_messages(
        &self,
        conversation_history_prefix: &[LlmHistoryEntry],
        live_context_message: &str,
        current_request_entry: &LlmHistoryEntry,
        trailing_user_message: Option<&str>,
    ) -> Vec<SemanticLlmMessage> {
        let mut messages = conversation_history_prefix
            .iter()
            .map(Self::semantic_message_from_history_entry)
            .collect::<Vec<_>>();

        messages.push(SemanticLlmMessage::text("system", live_context_message));
        messages.push(Self::semantic_message_from_history_entry(
            current_request_entry,
        ));

        if let Some(message) = trailing_user_message
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            messages.push(SemanticLlmMessage::text("user", message));
        }

        messages
    }

    pub(crate) fn build_next_step_decision_request_from_context(
        &self,
        prompt_context: &NextStepDecisionPromptEnvelope,
        prompt_context_value: &serde_json::Value,
        trailing_user_message: Option<&str>,
    ) -> Result<SemanticLlmRequest, AppError> {
        let live_context_message =
            prompt_context
                .live_context_message
                .as_deref()
                .ok_or_else(|| {
                    AppError::Internal(
                        "Next-step decision live context message missing".to_string(),
                    )
                })?;
        let config: SemanticLlmPromptConfig =
            render_template_json(AgentTemplates::STEP_DECISION, prompt_context_value)?;
        let system_prompt = render_prompt_text("next_step_decision_system", prompt_context_value)?;
        let messages = self.build_next_step_decision_messages(
            &prompt_context.conversation_history_prefix,
            live_context_message,
            &prompt_context.current_request_entry,
            trailing_user_message,
        );

        Ok(SemanticLlmRequest::from_config(
            system_prompt,
            messages,
            config,
        ))
    }

    async fn load_thread_mode_context(&self) -> Result<ThreadModeContext, AppError> {
        let exec_context = self.ctx.get_thread().await?;
        let is_conversation_thread =
            exec_context.thread_purpose == models::agent_thread::purpose::CONVERSATION;
        let is_coordinator_thread = exec_context.thread_purpose
            == models::agent_thread::purpose::COORDINATOR
            || self
                .active_thread_event
                .as_ref()
                .map(|event| Self::thread_event_implies_coordinator(&event.event_type))
                .unwrap_or(false);

        Ok(ThreadModeContext {
            exec_context,
            is_coordinator_thread,
            allows_user_interaction: is_conversation_thread,
        })
    }

    async fn build_conversation_prompt_context(&self) -> ConversationPromptContext {
        let mut conversation_history = self.get_conversation_history_for_llm().await;
        let current_request_entry = conversation_history.pop().unwrap_or_else(|| {
            LlmHistoryEntry::with_parts(
                "user",
                "user_message",
                None,
                vec![LlmHistoryPart::text(if self.user_request.trim().is_empty() {
                    "[No explicit current request message. Use the live context snapshot and recent history.]"
                } else {
                    self.user_request.as_str()
                })],
            )
        });

        ConversationPromptContext {
            conversation_history_prefix: conversation_history,
            current_request_entry,
        }
    }

    async fn load_board_prompt_context(
        &self,
        is_coordinator_thread: bool,
    ) -> Result<BoardPromptContext, AppError> {
        let active_assignment = self.load_active_assignment_prompt_item().await?;
        let active_board_item_id = active_assignment
            .as_ref()
            .map(|assignment| assignment.board_item_id)
            .or_else(|| {
                self.active_thread_event
                    .as_ref()
                    .and_then(|event| event.board_item_id)
            });
        let active_board_item = self
            .load_active_board_item_prompt_item(active_board_item_id)
            .await?;
        let active_board_item_assignments = self
            .load_active_board_item_assignments(active_board_item_id)
            .await;
        let recent_assignment_history =
            Self::recent_assignment_history(&active_board_item_assignments);
        let active_board_item_events = self
            .load_active_board_item_events(active_board_item_id)
            .await;
        let task_journal_tail = if active_board_item_id.is_some() {
            self.task_journal_tail_snippet().await?
        } else {
            None
        };

        Ok(BoardPromptContext {
            thread_assignment_queue: self.load_thread_assignment_queue().await,
            scoped_project_task_board_items: self
                .scoped_project_task_board_items(is_coordinator_thread, active_board_item.as_ref()),
            active_assignment,
            active_board_item,
            active_board_item_assignments,
            recent_assignment_history,
            active_board_item_events,
            task_journal_tail,
        })
    }

    async fn load_tool_prompt_context(
        &self,
        is_coordinator_thread: bool,
    ) -> Result<ToolPromptContext, AppError> {
        let available_tools = self.available_tools_for_mode().await;
        let discoverable_external_tool_names = self
            .ctx
            .agent
            .tools
            .iter()
            .filter(|tool| !matches!(tool.tool_type, models::AiToolType::Internal))
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>();
        let loaded_external_tool_names = self
            .loaded_external_tool_ids
            .iter()
            .filter_map(|tool_id| {
                self.ctx
                    .agent
                    .tools
                    .iter()
                    .find(|tool| tool.id == *tool_id)
                    .map(|tool| tool.name.clone())
            })
            .collect::<Vec<_>>();
        let tool_prompt_items = available_tools
            .iter()
            .map(ToolPromptItem::from_tool)
            .collect::<Vec<_>>();
        let knowledge_base_prompt_items = self
            .ctx
            .agent
            .knowledge_bases
            .iter()
            .map(KnowledgeBasePromptItem::from_knowledge_base)
            .collect::<Vec<_>>();
        let (system_skill_prompt_items, agent_skill_prompt_items) =
            self.filesystem.list_skill_prompt_items().await?;

        Ok(ToolPromptContext {
            available_sub_agents: if is_coordinator_thread {
                self.load_available_sub_agents().await
            } else {
                Vec::new()
            },
            tool_prompt_items,
            knowledge_base_prompt_items,
            system_skill_prompt_items,
            agent_skill_prompt_items,
            discoverable_external_tool_names,
            loaded_external_tool_names,
        })
    }

    async fn load_available_sub_agents(&self) -> Vec<dto::json::SubAgentPromptInfo> {
        let Some(sub_agent_ids) = &self.ctx.agent.sub_agents else {
            return Vec::new();
        };
        if sub_agent_ids.is_empty() {
            return Vec::new();
        }

        queries::GetAiAgentsByIdsQuery::new(self.ctx.agent.deployment_id, sub_agent_ids.clone())
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

    async fn load_active_assignment_prompt_item(
        &self,
    ) -> Result<Option<dto::json::ProjectTaskBoardAssignmentPromptItem>, AppError> {
        let Some(assignment_id) = self.active_thread_event.as_ref().and_then(|event| {
            event
                .assignment_execution_payload()
                .map(|payload| payload.assignment_id)
        }) else {
            return Ok(None);
        };

        Ok(
            GetProjectTaskBoardItemAssignmentByIdQuery::new(assignment_id)
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await
                .ok()
                .flatten()
                .map(|assignment| {
                    let mut item = Self::assignment_prompt_item_from_row(&assignment);
                    item.mode = Some("assignment_execution".to_string());
                    item
                }),
        )
    }

    async fn load_active_board_item_prompt_item(
        &self,
        active_board_item_id: Option<i64>,
    ) -> Result<Option<dto::json::ProjectTaskBoardPromptItem>, AppError> {
        let Some(item_id) = active_board_item_id else {
            return Ok(None);
        };

        Ok(queries::GetProjectTaskBoardItemByIdQuery::new(item_id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
            .ok()
            .flatten()
            .map(|item| Self::project_task_board_item_to_prompt_item(&item)))
    }

    async fn load_thread_assignment_queue(
        &self,
    ) -> Vec<dto::json::ProjectTaskBoardAssignmentPromptItem> {
        queries::ListAssignmentsForThreadQuery::new(self.ctx.thread_id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|assignment| {
                !matches!(
                    assignment.status.as_str(),
                    models::project_task_board::assignment_status::COMPLETED
                        | models::project_task_board::assignment_status::CANCELLED
                        | models::project_task_board::assignment_status::REJECTED
                )
            })
            .map(|assignment| Self::assignment_prompt_item_from_row(&assignment))
            .collect()
    }

    async fn load_active_board_item_assignments(
        &self,
        active_board_item_id: Option<i64>,
    ) -> Vec<dto::json::ProjectTaskBoardAssignmentPromptItem> {
        let Some(item_id) = active_board_item_id else {
            return Vec::new();
        };

        queries::ListProjectTaskBoardItemAssignmentsQuery::new(item_id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|assignment| Self::assignment_prompt_item_from_row(&assignment))
            .collect()
    }

    async fn load_active_board_item_events(
        &self,
        active_board_item_id: Option<i64>,
    ) -> Vec<dto::json::ProjectTaskBoardItemEventPromptItem> {
        let Some(item_id) = active_board_item_id else {
            return Vec::new();
        };

        queries::ListProjectTaskBoardItemEventsQuery::new(item_id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
            .unwrap_or_default()
            .into_iter()
            .take(10)
            .map(Self::board_item_event_prompt_item)
            .collect()
    }

    fn recent_assignment_history(
        assignments: &[dto::json::ProjectTaskBoardAssignmentPromptItem],
    ) -> Vec<dto::json::ProjectTaskBoardAssignmentPromptItem> {
        if assignments.len() > 5 {
            assignments[assignments.len() - 5..].to_vec()
        } else {
            assignments.to_vec()
        }
    }

    fn scoped_project_task_board_items(
        &self,
        is_coordinator_thread: bool,
        active_board_item: Option<&dto::json::ProjectTaskBoardPromptItem>,
    ) -> Vec<dto::json::ProjectTaskBoardPromptItem> {
        if !is_coordinator_thread {
            return Vec::new();
        }

        active_board_item
            .map(|item| {
                self.project_task_board_items
                    .iter()
                    .filter(|candidate| candidate.task_key == item.task_key)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| self.project_task_board_items.clone())
    }

    pub(super) async fn generate_next_step_decision(
        &mut self,
    ) -> Result<NextStepDecision, AppError> {
        let context_json = self.build_next_step_decision_prompt_context_json().await?;
        let prompt_context: NextStepDecisionPromptEnvelope =
            serde_json::from_value(context_json.clone()).map_err(|e| {
                AppError::Internal(format!(
                    "Failed to deserialize step decision prompt context: {e}"
                ))
            })?;
        let request = self.build_next_step_decision_request_from_context(
            &prompt_context,
            &context_json,
            None,
        )?;

        self.refresh_long_think_credits();
        if self.long_think_mode_active && !self.long_think_credits_available() {
            self.long_think_mode_active = false;
        }

        let using_high_thinking_mode = self.long_think_mode_active;
        let llm = self.create_strong_llm().await?;
        let output = llm
            .generate_structured_from_prompt::<NextStepDecision>(
                request,
                Some(PromptCacheRequest {
                    cache_key: self.next_step_decision_cache_key(llm.model_name()),
                    ttl_secs: STEP_DECISION_CACHE_TTL_SECS,
                    live_tail_count: 2,
                    prior_state: self.next_step_decision_cache_state.clone(),
                    reuse_only: false,
                }),
            )
            .await?;
        let (decision, usage_metadata, cache_state) =
            (output.value, output.usage_metadata, output.cache_state);

        self.persist_next_step_decision_cache_state(cache_state)
            .await?;

        let callable_tool_names = self
            .available_tools_for_mode()
            .await
            .into_iter()
            .filter(|tool| !matches!(tool.name.as_str(), "search_tools" | "load_tools"))
            .map(|tool| tool.name)
            .collect::<HashSet<_>>();

        validate_next_step_decision(
            &NextStepDecisionValidationContext {
                long_think_mode_active: self.long_think_mode_active,
                long_think_credits_available: self.long_think_credits_available(),
                long_think_input_tokens_available: self
                    .long_think_credit_snapshot
                    .input_tokens_available,
                long_think_input_token_budget: 2_000_000,
                long_think_output_tokens_available: self
                    .long_think_credit_snapshot
                    .output_tokens_available,
                long_think_output_token_budget: 300_000,
                can_abort_current_assignment_execution: self
                    .can_abort_current_assignment_execution(),
                callable_tool_names,
                has_recent_startaction: self.active_startaction_directive.is_some(),
            },
            &decision,
        )?;
        let (_selected_tool_count, _selected_tools) =
            Self::selected_tool_call_log_fields(&decision);
        self.record_llm_usage_for_compaction(usage_metadata.as_ref());
        if using_high_thinking_mode {
            self.consume_long_think_credits(usage_metadata.as_ref());
            self.long_think_mode_active = false;
        }

        if !matches!(
            decision.next_step,
            dto::json::agent_executor::NextStep::Steer
        ) {
            let conversation = self
                .create_conversation_with_metadata(
                    models::ConversationContent::SystemDecision {
                        step: Self::next_step_decision_key(&decision.next_step).to_string(),
                        reasoning: decision.reasoning.clone(),
                        confidence: decision.confidence as f32,
                    },
                    models::ConversationMessageType::SystemDecision,
                    Self::next_step_decision_metadata(&decision),
                )
                .await?;
            self.conversations.push(conversation.clone());
            let _ = self
                .channel
                .send(dto::json::StreamEvent::ConversationMessage(conversation))
                .await;
        }

        Ok(decision)
    }
}
