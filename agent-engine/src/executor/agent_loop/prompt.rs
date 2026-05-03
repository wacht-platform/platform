use super::core::AgentExecutor;

use common::error::AppError;
use dto::json::{
    AgentLoopContext, AgentLoopConversationContext, AgentLoopPromptEnvelope,
    AgentLoopResourceContext, AgentLoopRuntimeContext, AgentLoopTaskContext,
    AgentLoopThreadContext, KnowledgeBasePromptItem, LlmHistoryEntry, LlmHistoryPart,
    SkillPromptItem, ToolPromptItem,
};

use crate::llm::{SemanticLlmMessage, SemanticLlmPromptConfig, SemanticLlmRequest};
use templatekit::{AgentTemplates, render_prompt_text, render_template_only};
use queries::GetProjectTaskBoardItemAssignmentByIdQuery;
const STEER_VISIBILITY_NUDGE_WINDOW: usize = 4;

pub(crate) struct ThreadModeContext {
    pub(crate) exec_context: models::AgentThreadState,
    pub(crate) is_coordinator_thread: bool,
    pub(crate) allows_user_interaction: bool,
}

struct ConversationPromptContext {
    conversation_history_prefix: Vec<LlmHistoryEntry>,
    current_request_entry: LlmHistoryEntry,
}

pub(crate) struct BoardPromptContext {
    pub(crate) active_assignment: Option<dto::json::ProjectTaskBoardAssignmentPromptItem>,
    pub(crate) active_board_item: Option<dto::json::ProjectTaskBoardPromptItem>,
    pub(crate) active_board_item_assignments: Vec<dto::json::ProjectTaskBoardAssignmentPromptItem>,
    pub(crate) recent_assignment_history: Vec<dto::json::ProjectTaskBoardAssignmentPromptItem>,
    pub(crate) task_journal_tail: Option<String>,
    pub(crate) thread_assignment_queue: Vec<dto::json::ProjectTaskBoardAssignmentPromptItem>,
    pub(crate) scoped_project_task_board_items: Vec<dto::json::ProjectTaskBoardPromptItem>,
}

pub(crate) struct ToolPromptContext {
    pub(crate) tool_prompt_items: Vec<ToolPromptItem>,
    pub(crate) knowledge_base_prompt_items: Vec<KnowledgeBasePromptItem>,
    pub(crate) system_skill_prompt_items: Vec<SkillPromptItem>,
    pub(crate) agent_skill_prompt_items: Vec<SkillPromptItem>,
    pub(crate) available_sub_agents: Vec<dto::json::SubAgentPromptInfo>,
    pub(crate) discoverable_external_tool_names: Vec<String>,
    pub(crate) loaded_external_tool_names: Vec<String>,
    pub(crate) connected_external_integrations: Vec<String>,
}

impl AgentExecutor {
    fn steer_visibility_nudge(&self, allows_user_interaction: bool) -> Option<String> {
        if !allows_user_interaction {
            return None;
        }

        let mut recent_visible_messages = Vec::new();
        for conv in self.conversations.iter().rev() {
            match conv.message_type {
                models::ConversationMessageType::UserMessage
                | models::ConversationMessageType::ClarificationResponse => {
                    if recent_visible_messages.is_empty() {
                        return None;
                    }
                    break;
                }
                models::ConversationMessageType::Steer
                | models::ConversationMessageType::ApprovalRequest
                | models::ConversationMessageType::ApprovalResponse
                | models::ConversationMessageType::ClarificationRequest
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
            "No visible message was sent to the user in the last 4 conversation-visible steps. Before continuing another non-trivial action run, emit a short progress text alongside your next tool call(s) to keep the user informed. Skip this only if the next step is a tiny immediate read/search/list action."
                .to_string(),
        )
    }

    pub(crate) async fn fetch_all_prompt_caches(
        &self,
    ) -> Result<
        (
            ThreadModeContext,
            models::ThreadTaskGraphSnapshot,
            BoardPromptContext,
            ToolPromptContext,
        ),
        AppError,
    > {
        let thread_mode = self.load_thread_mode_context().await?;
        let is_coordinator = thread_mode.is_coordinator_thread;

        let (task_graph, board_context, tool_context) = tokio::try_join!(
            self.fetch_task_graph_snapshot(),
            self.load_board_prompt_context(is_coordinator),
            self.load_tool_prompt_context(is_coordinator),
        )?;

        Ok((thread_mode, task_graph, board_context, tool_context))
    }

    pub(crate) async fn build_agent_loop_context_json(
        &mut self,
    ) -> Result<serde_json::Value, AppError> {
        let thread_mode = match self.thread_mode_cache.take() {
            Some(v) => v,
            None => self.load_thread_mode_context().await?,
        };
        let is_coordinator = thread_mode.is_coordinator_thread;

        let task_graph = self.ensure_task_graph_snapshot().await?;
        let task_graph_view = Self::render_task_graph_view(&task_graph);

        let (board_context, conversation_context, tool_context) = match (
            self.board_context_cache.take(),
            self.tool_context_cache.take(),
        ) {
            (Some(board), Some(tool)) => {
                let conv = self.build_conversation_prompt_context().await;
                (board, conv, tool)
            }
            _ => tokio::try_join!(
                self.load_board_prompt_context(is_coordinator),
                async {
                    Ok::<_, AppError>(self.build_conversation_prompt_context().await)
                },
                self.load_tool_prompt_context(is_coordinator),
            )?,
        };
        let context = AgentLoopContext {
            runtime: AgentLoopRuntimeContext {
                current_datetime_utc: chrono::Utc::now()
                    .format("%Y-%m-%d %H:%M:%S UTC")
                    .to_string(),
                iteration_info: dto::json::IterationInfo {
                    current_iteration: self.current_iteration.max(1),
                    max_iterations: 50,
                },
                steer_visibility_nudge: self
                    .steer_visibility_nudge(thread_mode.allows_user_interaction),
            },
            conversation: AgentLoopConversationContext {
                user_request: self.user_request.clone(),
                triggering_event: self
                    .active_thread_event
                    .as_ref()
                    .map(Self::thread_event_prompt_item),
                input_safety_signals: self.derive_input_safety_signals(),
            },
            thread: AgentLoopThreadContext {
                id: self.ctx.thread_id,
                title: thread_mode.exec_context.title.clone(),
                purpose: if thread_mode.is_coordinator_thread {
                    models::agent_thread::purpose::COORDINATOR.to_string()
                } else {
                    thread_mode.exec_context.thread_purpose.clone()
                },
                responsibility: thread_mode.exec_context.responsibility.clone(),
            },
            resources: AgentLoopResourceContext {
                available_tools: tool_context.tool_prompt_items,
                available_knowledge_bases: tool_context.knowledge_base_prompt_items,
                available_system_skills: tool_context.system_skill_prompt_items,
                available_agent_skills: tool_context.agent_skill_prompt_items,
                available_sub_agents: tool_context.available_sub_agents,
            },
            task: AgentLoopTaskContext {
                project_task_board_items: board_context.scoped_project_task_board_items,
                active_board_item: board_context.active_board_item,
                active_assignment: board_context.active_assignment,
                active_board_item_assignments: board_context.active_board_item_assignments,
                recent_assignment_history: board_context.recent_assignment_history,
                task_journal_tail: board_context.task_journal_tail,
                thread_assignment_queue: board_context.thread_assignment_queue,
                task_graph_view,
            },
        };

        let mut prompt_context = AgentLoopPromptEnvelope {
            base: context,
            agent_name: self.ctx.agent.name.clone(),
            agent_description: self.ctx.agent.description.clone(),
            conversation_history_prefix: conversation_context.conversation_history_prefix.clone(),
            current_request_entry: conversation_context.current_request_entry,
            discoverable_external_tool_names: tool_context.discoverable_external_tool_names,
            loaded_external_tool_names: tool_context.loaded_external_tool_names,
            connected_external_integrations: tool_context.connected_external_integrations,
            custom_system_instructions: self
                .system_instructions
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            live_context_message: None,
        };

        let mut prompt_context_value = serde_json::to_value(&prompt_context)?;
        Self::annotate_live_task_context_flag(&mut prompt_context_value);

        let live_context_message = render_template_only(
            AgentTemplates::AGENT_LOOP_LIVE_CONTEXT,
            &prompt_context_value,
        )
        .map_err(|e| {
            AppError::Internal(format!(
                "Failed to render agent loop live context template: {e}"
            ))
        })?;

        prompt_context.live_context_message = Some(live_context_message);

        Ok(serde_json::to_value(&prompt_context)?)
    }

    pub(crate) fn build_agent_loop_messages(
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

    pub(crate) fn build_agent_loop_request(
        &self,
        prompt_context: &AgentLoopPromptEnvelope,
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
        let reasoning_effort = if self.repeated_tool_call_count >= 2 {
            "medium"
        } else {
            "low"
        };
        let config = SemanticLlmPromptConfig {
            response_json_schema: serde_json::json!({}),
            temperature: None,
            max_output_tokens: Some(4096),
            reasoning_effort: Some(reasoning_effort.to_string()),
        };
        let system_prompt = render_prompt_text(self.system_prompt_name(), prompt_context_value)?;
        let messages = self.build_agent_loop_messages(
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

    fn annotate_live_task_context_flag(value: &mut serde_json::Value) {
        let triggering_event_present = value
            .get("conversation")
            .and_then(|v| v.get("triggering_event"))
            .map(|v| !v.is_null())
            .unwrap_or(false);

        let task = match value.get_mut("task").and_then(|v| v.as_object_mut()) {
            Some(t) => t,
            None => return,
        };

        let has_field = |key: &str| match task.get(key) {
            None | Some(serde_json::Value::Null) => false,
            Some(serde_json::Value::Array(a)) => !a.is_empty(),
            Some(serde_json::Value::String(s)) => !s.is_empty(),
            Some(serde_json::Value::Object(o)) => !o.is_empty(),
            Some(_) => true,
        };

        let has_any = triggering_event_present
            || has_field("active_assignment")
            || has_field("active_board_item")
            || has_field("active_board_item_assignments")
            || has_field("recent_assignment_history")
            || has_field("task_journal_tail")
            || has_field("thread_assignment_queue");

        task.insert("has_live_context".to_string(), serde_json::Value::Bool(has_any));
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
        let task_journal_tail = if active_board_item.is_some() {
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
            (Vec::new(), Vec::new());

        let connected_external_integrations = self.load_connected_external_integrations().await;

        let available_sub_agents = if is_coordinator_thread {
            self.load_available_sub_agents().await
        } else {
            Vec::new()
        };

        Ok(ToolPromptContext {
            available_sub_agents,
            tool_prompt_items,
            knowledge_base_prompt_items,
            system_skill_prompt_items,
            agent_skill_prompt_items,
            discoverable_external_tool_names,
            loaded_external_tool_names,
            connected_external_integrations,
        })
    }

    async fn load_connected_external_integrations(&self) -> Vec<String> {
        use queries::composio::{GetActiveComposioSlugsForActorQuery, GetComposioSettingsQuery};
        let deployment_id = self.ctx.agent.deployment_id;
        let Ok(thread) = self.ctx.get_thread().await else {
            return Vec::new();
        };
        let actor_id = thread.actor_id;

        let Ok(Some(settings)) = GetComposioSettingsQuery::new(deployment_id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
        else {
            return Vec::new();
        };
        if !settings.enabled {
            return Vec::new();
        }
        let enabled_apps: Vec<models::ComposioEnabledApp> =
            serde_json::from_value(settings.enabled_apps).unwrap_or_default();
        let candidate_slugs: Vec<String> = enabled_apps.into_iter().map(|a| a.slug).collect();
        if candidate_slugs.is_empty() {
            return Vec::new();
        }

        GetActiveComposioSlugsForActorQuery::new(deployment_id, actor_id, candidate_slugs)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
            .unwrap_or_default()
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
}
