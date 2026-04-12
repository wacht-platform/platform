use crate::filesystem::{shell::ShellExecutor, AgentFilesystem};
use crate::tools::ToolExecutor;

use common::error::AppError;
use dto::json::agent_executor::{SnapshotExecutionStateParams, StartActionDirective, ToolCallBrief};
use dto::json::{ProjectTaskBoardPromptItem, StreamEvent};
use models::{AiTool, AiToolConfiguration, AiToolType, InternalToolConfiguration};
use models::{ConversationRecord, MemoryRecord, ThreadEvent, ThreadExecutionState};
use queries::ListActiveApprovalGrantsForThreadQuery;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub enum ResumeContext {
    ApprovalResponse(Vec<dto::json::deployment::ToolApprovalSelection>),
}

pub struct AgentExecutor {
    pub(crate) ctx: std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
    pub(crate) conversations: Vec<ConversationRecord>,
    pub(crate) tool_executor: ToolExecutor,
    pub(crate) channel: tokio::sync::mpsc::Sender<StreamEvent>,
    pub(crate) memories: Vec<MemoryRecord>,
    pub(crate) user_request: String,
    pub(crate) system_instructions: Option<String>,
    pub(crate) filesystem: AgentFilesystem,
    pub(crate) shell: ShellExecutor,
    pub(crate) current_iteration: usize,
    pub(crate) long_think_mode_active: bool,
    pub(crate) long_think_credit_snapshot: models::LongThinkCreditSnapshot,
    pub(crate) loaded_external_tool_ids: Vec<i64>,
    pub(crate) next_step_decision_cache_state: Option<models::PromptCacheState>,
    pub(crate) active_startaction_directive: Option<StartActionDirective>,
    pub(crate) active_tool_call_brief: Option<ToolCallBrief>,
    pub(crate) project_task_board_items: Vec<ProjectTaskBoardPromptItem>,
    pub(crate) project_task_board_id: Option<i64>,
    pub(crate) approved_always_tool_ids: HashSet<i64>,
    pub(crate) task_graph_snapshot: Option<serde_json::Value>,
    pub(crate) last_decision_signature: Option<String>,
    pub(crate) repeated_decision_count: usize,
    pub(crate) active_thread_event: Option<ThreadEvent>,
    pub(crate) is_conversation_thread: bool,
    pub(crate) is_coordinator_thread: bool,
    pub(crate) task_journal_start_hash: Option<String>,
    pub(crate) conversation_compaction_state: models::ConversationCompactionState,
    pub(crate) snapshot_execution_state_requested: bool,
}

pub struct AgentExecutorBuilder {
    ctx: std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
    channel: tokio::sync::mpsc::Sender<StreamEvent>,
}

impl AgentExecutorBuilder {
    pub fn new(
        ctx: std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Self {
        Self { ctx, channel }
    }

    pub async fn build(self) -> Result<AgentExecutor, AppError> {
        let execution_context = self.ctx.clone();
        let thread = self.ctx.get_thread().await?;

        let tool_executor =
            ToolExecutor::new(execution_context.clone()).with_channel(self.channel.clone());
        let execution_id = self.ctx.app_state.sf.next_id()?.to_string();

        let filesystem = AgentFilesystem::new(
            &self.ctx.app_state,
            &self.ctx.agent.deployment_id.to_string(),
            &self.ctx.agent.id.to_string(),
            &thread.project_id.to_string(),
            &self.ctx.thread_id.to_string(),
            &execution_id,
        )
        .await?;

        filesystem.initialize().await?;

        let shell = ShellExecutor::new(filesystem.execution_root());

        Self::link_knowledge_bases(&filesystem, &self.ctx.agent.knowledge_bases).await?;

        let context = execution_context.get_thread().await?;
        let is_conversation_thread =
            context.thread_purpose == models::agent_thread::purpose::CONVERSATION;
        let is_coordinator_thread = context.thread_purpose
            == models::agent_thread::purpose::COORDINATOR
            || context.title.eq_ignore_ascii_case("coordinator")
            || context
                .responsibility
                .as_deref()
                .map(|value| {
                    value.eq_ignore_ascii_case("project coordinator")
                        || value.eq_ignore_ascii_case("coordinator")
                })
                .unwrap_or(false);
        let internal_tools = super::tools::definitions::internal_tools();
        let active_approvals = ListActiveApprovalGrantsForThreadQuery::new(
            self.ctx.agent.deployment_id,
            self.ctx.thread_id,
        )
        .execute_with_db(self.ctx.app_state.db_router.writer())
        .await?;
        let approved_always_tool_ids = active_approvals
            .iter()
            .filter(|approval| approval.grant_scope != models::approval::grant_scope::ONCE)
            .map(|approval| approval.tool_id)
            .collect::<HashSet<_>>();

        let mut current_tools = self
            .ctx
            .agent
            .tools
            .clone()
            .into_iter()
            .filter(|tool| {
                if tool.tool_type != AiToolType::Internal {
                    return true;
                }

                Self::should_inject_internal_tool(is_coordinator_thread, &tool.name)
            })
            .collect::<Vec<_>>();
        for (name, desc, tool_type, schema) in internal_tools {
            if !Self::should_inject_internal_tool(is_coordinator_thread, name) {
                continue;
            }
            if !current_tools.iter().any(|t| t.name == name) {
                current_tools.push(AiTool {
                    id: -1,
                    name: name.to_string(),
                    description: Some(desc.to_string()),
                    tool_type: AiToolType::Internal,
                    deployment_id: self.ctx.agent.deployment_id,
                    requires_user_approval: false,
                    configuration: AiToolConfiguration::Internal(InternalToolConfiguration {
                        tool_type,
                        input_schema: Some(schema),
                    }),
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                });
            }
        }

        let mut agent_with_tools = self.ctx.agent.clone();
        agent_with_tools.tools = current_tools.clone();

        let execution_context = execution_context.with_agent(agent_with_tools);

        let mut executor = AgentExecutor {
            ctx: execution_context,
            tool_executor,
            user_request: String::new(),
            channel: self.channel,
            memories: Vec::new(),
            conversations: Vec::new(),
            system_instructions: None,
            filesystem,
            shell,
            current_iteration: 0,
            long_think_mode_active: false,
            long_think_credit_snapshot: models::LongThinkCreditSnapshot::default(),
            loaded_external_tool_ids: Vec::new(),
            next_step_decision_cache_state: None,
            active_startaction_directive: None,
            active_tool_call_brief: None,
            project_task_board_items: Vec::new(),
            project_task_board_id: None,
            approved_always_tool_ids,
            task_graph_snapshot: None,
            last_decision_signature: None,
            repeated_decision_count: 0,
            active_thread_event: None,
            is_conversation_thread,
            is_coordinator_thread,
            task_journal_start_hash: None,
            conversation_compaction_state: models::ConversationCompactionState::default(),
            snapshot_execution_state_requested: false,
        };

        executor.system_instructions = context.system_instructions.clone();

        if let Some(state) = context.execution_state {
            executor.restore_from_state(state)?;
        }

        if executor.is_coordinator_thread {
            executor.refresh_project_task_board_items().await?;
        }

        executor.ensure_task_graph_snapshot().await?;

        Ok(executor)
    }

    fn should_inject_internal_tool(is_coordinator_thread: bool, tool_name: &str) -> bool {
        if is_coordinator_thread {
            return matches!(
                tool_name,
                "list_threads"
                    | "create_thread"
                    | "update_thread"
                    | "create_project_task"
                    | "update_project_task"
                    | "assign_project_task"
                    | "snapshot_execution_state"
                    | "sleep"
            );
        }

        true
    }

    async fn link_knowledge_bases(
        filesystem: &AgentFilesystem,
        knowledge_bases: &[models::AiKnowledgeBase],
    ) -> Result<(), AppError> {
        for kb in knowledge_bases {
            filesystem
                .link_knowledge_base(&kb.id.to_string(), &kb.name)
                .await
                .map_err(|error| {
                    AppError::Internal(format!(
                        "failed to link knowledge base '{}' ({}): {}",
                        kb.name, kb.id, error
                    ))
                })?;
        }

        Ok(())
    }
}

impl AgentExecutor {
    pub(crate) async fn execute_snapshot_execution_state(
        &mut self,
        params: SnapshotExecutionStateParams,
    ) -> Result<serde_json::Value, AppError> {
        self.snapshot_execution_state_requested = true;
        Ok(serde_json::json!({
            "success": true,
            "tool": "snapshot_execution_state",
            "message": "Local execution checkpoint flag set for this run.",
            "reason": params.reason,
            "task_graph_present": self.task_graph_snapshot.is_some(),
            "task_graph_has_unfinished_nodes": self.active_task_graph_has_unfinished_nodes(),
            "active_startaction_present": self.active_startaction_directive.is_some(),
            "active_tool_call_brief_present": self.active_tool_call_brief.is_some(),
        }))
    }
}

impl AgentExecutor {
    pub(crate) fn thread_event_implies_coordinator(event_type: &str) -> bool {
        matches!(
            event_type,
            models::thread_event::event_type::TASK_ROUTING
                | models::thread_event::event_type::ASSIGNMENT_OUTCOME_REVIEW
        )
    }

    pub(crate) fn effective_is_coordinator_thread(&self) -> bool {
        self.is_coordinator_thread
            || self
                .active_thread_event
                .as_ref()
                .map(|event| Self::thread_event_implies_coordinator(&event.event_type))
                .unwrap_or(false)
    }
}

impl AgentExecutor {
    pub async fn new(
        ctx: std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Self, AppError> {
        AgentExecutorBuilder::new(ctx, channel).build().await
    }

    pub(super) fn restore_from_state(
        &mut self,
        state: ThreadExecutionState,
    ) -> Result<(), AppError> {
        self.long_think_mode_active = false;
        self.long_think_credit_snapshot = state.long_think_credit_snapshot;
        self.loaded_external_tool_ids = state.loaded_external_tool_ids;
        self.next_step_decision_cache_state = state.prompt_caches.step_decision;
        self.active_startaction_directive = state
            .active_startaction_directive
            .and_then(|value| serde_json::from_value::<StartActionDirective>(value).ok());
        self.active_tool_call_brief = state
            .active_tool_call_brief
            .and_then(|value| serde_json::from_value::<ToolCallBrief>(value).ok());
        self.project_task_board_items.clear();
        self.task_journal_start_hash = state.task_journal_start_hash;
        self.conversation_compaction_state = state.conversation_compaction_state;

        Ok(())
    }

    pub(super) fn can_write_project_task_board_in_current_mode(&self) -> bool {
        self.effective_is_coordinator_thread()
            || self
                .active_thread_event
                .as_ref()
                .map(|event| {
                    event.event_type == models::thread_event::event_type::ASSIGNMENT_EXECUTION
                })
                .unwrap_or(false)
    }

    pub(super) fn can_create_project_task_in_current_mode(&self) -> bool {
        self.effective_is_coordinator_thread()
            || self.is_conversation_thread
            || self
                .active_thread_event
                .as_ref()
                .map(|event| {
                    matches!(
                        event.event_type.as_str(),
                        models::thread_event::event_type::USER_MESSAGE_RECEIVED
                            | models::thread_event::event_type::USER_INPUT_RECEIVED
                            | models::thread_event::event_type::APPROVAL_RESPONSE_RECEIVED
                    )
                })
                .unwrap_or(false)
    }

    pub(super) fn normal_mode_disallowed_tool(tool_name: &str) -> bool {
        matches!(
            tool_name,
            "create_thread"
                | "update_thread"
                | "create_project_task"
                | "update_project_task"
                | "assign_project_task"
        )
    }

    pub(super) fn tool_allowed_in_current_mode(&self, tool_name: &str) -> bool {
        if self.effective_is_coordinator_thread() {
            return matches!(
                tool_name,
                "create_thread"
                    | "update_thread"
                    | "create_project_task"
                    | "update_project_task"
                    | "assign_project_task"
                    | "list_threads"
                    | "sleep"
            );
        }

        if tool_name == "list_threads" {
            return true;
        }

        if tool_name == "update_project_task" {
            return self.can_write_project_task_board_in_current_mode();
        }

        if tool_name == "create_project_task" {
            return self.can_create_project_task_in_current_mode();
        }

        !Self::normal_mode_disallowed_tool(tool_name)
    }
}
