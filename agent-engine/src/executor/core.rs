use crate::filesystem::{shell::ShellExecutor, AgentFilesystem};
use crate::tools::ToolExecutor;

use common::error::AppError;
use dto::json::{ProjectTaskBoardPromptItem, StreamEvent};
use models::{
    AiTool, AiToolConfiguration, AiToolType, ImmediateContext, InternalToolConfiguration,
};
use models::{ConversationRecord, MemoryRecord, ThreadEvent};
use queries::ListActiveApprovalGrantsForThreadQuery;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub enum ResumeContext {
    ApprovalResponse(Vec<dto::json::deployment::ToolApprovalSelection>),
}

pub struct AgentExecutor {
    pub(crate) ctx:
        std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
    pub(crate) conversations: Vec<ConversationRecord>,
    pub(crate) routing_events: Vec<models::TaskRoutingEvent>,
    pub(crate) task_thread_meta: Vec<models::TaskThreadMeta>,
    pub(crate) tool_executor: ToolExecutor,
    pub(crate) channel: tokio::sync::mpsc::Sender<StreamEvent>,
    pub(crate) memories: Vec<MemoryRecord>,
    pub(crate) user_request: String,
    pub(crate) system_instructions: Option<String>,
    pub(crate) filesystem: AgentFilesystem,
    pub(crate) shell: ShellExecutor,
    pub(crate) current_iteration: usize,
    pub(crate) loaded_external_tool_ids: Vec<i64>,
    pub(crate) virtual_tool_cache: std::collections::HashMap<i64, models::AiTool>,
    pub(crate) project_task_board_items: Vec<ProjectTaskBoardPromptItem>,
    pub(crate) project_task_board_id: Option<i64>,
    pub(crate) approved_always_tool_ids: HashSet<i64>,
    pub(crate) task_graph_snapshot: Option<models::ThreadTaskGraphSnapshot>,
    pub(crate) thread_mode_cache: Option<crate::executor::agent_loop::prompt::ThreadModeContext>,
    pub(crate) board_context_cache: Option<crate::executor::agent_loop::prompt::BoardPromptContext>,
    pub(crate) tool_context_cache: Option<crate::executor::agent_loop::prompt::ToolPromptContext>,
    pub(crate) active_thread_event: Option<ThreadEvent>,
    pub(crate) active_schedule_carryover: Option<models::ScheduleCarryover>,
    pub(crate) is_conversation_thread: bool,
    pub(crate) is_coordinator_thread: bool,
    pub(crate) is_review_thread: bool,
    pub(crate) task_journal_start_hash: Option<String>,
    pub(crate) conversation_compaction_state: models::ConversationCompactionState,
    pub(crate) pending_question: Option<models::PendingQuestion>,
    pub(crate) consecutive_note_count: usize,
    pub(crate) last_tool_call_signature: Option<String>,
    pub(crate) repeated_tool_call_count: usize,
    pub(crate) terminal_review_continue_count: usize,
    pub(crate) preloaded_immediate_context: Option<ImmediateContext>,
    pub(crate) budget: super::budget::BudgetCounter,
}

pub struct PreparedExecutor {
    ctx: std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
    is_conversation_thread: bool,
    is_coordinator_thread: bool,
    is_review_thread: bool,
    approved_always_tool_ids: HashSet<i64>,
    project_task_board_items: Vec<ProjectTaskBoardPromptItem>,
    project_task_board_id: Option<i64>,
    system_instructions: Option<String>,
    loaded_external_tool_ids: Vec<i64>,
    virtual_tool_cache: HashMap<i64, AiTool>,
    task_journal_start_hash: Option<String>,
    conversation_compaction_state: models::ConversationCompactionState,
    pending_question: Option<models::PendingQuestion>,
    immediate_context: ImmediateContext,
}

pub struct AgentExecutorBuilder;

impl AgentExecutorBuilder {
    pub async fn prepare(
        ctx: std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
        board_item_id: Option<i64>,
    ) -> Result<PreparedExecutor, AppError> {
        let context = ctx.get_thread().await?;
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
        let is_review_thread = context.thread_purpose == models::agent_thread::purpose::REVIEW;
        let is_service_thread = !is_coordinator_thread
            && !is_conversation_thread
            && matches!(
                context.thread_purpose.as_str(),
                models::agent_thread::purpose::EXECUTION | models::agent_thread::purpose::REVIEW
            );

        let actor_id = context.actor_id;
        let deployment_id = ctx.agent.deployment_id;
        let thread_id = ctx.thread_id;
        let db = ctx.app_state.db_router.writer();

        let approvals_query = ListActiveApprovalGrantsForThreadQuery::new(deployment_id, thread_id);
        let approvals_fut = approvals_query.execute_with_db(db);
        let board_fut = async {
            if is_coordinator_thread {
                super::project::load_project_task_board_state(&ctx)
                    .await
                    .map(|(board_id, items)| (Some(board_id), items))
            } else {
                Ok::<_, AppError>((None, Vec::new()))
            }
        };
        let immediate_ctx_fut =
            super::context::memory_context::load_immediate_context(&ctx, board_item_id);
        let mcp_pipeline_fut = async {
            let mut mcp_connections =
                queries::GetActorMcpConnectionsQuery::new(deployment_id, actor_id)
                    .execute_with_db(db)
                    .await
                    .unwrap_or_default();

            let refresh_targets: Vec<usize> = mcp_connections
                .iter()
                .enumerate()
                .filter(|(_, c)| crate::tools::mcp::connection_needs_refresh(c))
                .map(|(idx, _)| idx)
                .collect();
            let refresh_results =
                futures::future::join_all(refresh_targets.into_iter().map(|idx| {
                    let conn_ref = &mcp_connections[idx];
                    async move {
                        let result = crate::tools::mcp::refresh_connection_metadata(conn_ref).await;
                        (idx, result)
                    }
                }))
                .await;

            let persist_futures = refresh_results
                .into_iter()
                .filter_map(|(idx, maybe_meta)| maybe_meta.map(|m| (idx, m)))
                .map(|(idx, new_meta)| {
                    let meta_for_persist = serde_json::to_value(&new_meta).ok();
                    let server_id = mcp_connections[idx].server.id;
                    mcp_connections[idx].connection_metadata = Some(new_meta);
                    async move {
                        if let Some(meta_json) = meta_for_persist {
                            let _ = queries::UpdateActorMcpConnectionMetadataQuery::new(
                                deployment_id,
                                actor_id,
                                server_id,
                                meta_json,
                            )
                            .execute_with_db(db)
                            .await;
                        }
                    }
                })
                .collect::<Vec<_>>();
            futures::future::join_all(persist_futures).await;

            crate::tools::mcp::discover_mcp_tools_for_actor(mcp_connections, deployment_id).await
        };

        let (active_approvals, mcp_tools, board_state, immediate_context) = tokio::join!(
            approvals_fut,
            mcp_pipeline_fut,
            board_fut,
            immediate_ctx_fut
        );
        let active_approvals = active_approvals?;
        let (project_task_board_id, project_task_board_items) = board_state?;
        let immediate_context = immediate_context?;

        let approved_always_tool_ids = active_approvals
            .iter()
            .filter(|approval| approval.grant_scope != models::approval::grant_scope::ONCE)
            .map(|approval| approval.tool_id)
            .collect::<HashSet<_>>();

        let internal_tools = super::tools::definitions::internal_tools();
        let mut current_tools = ctx
            .agent
            .tools
            .clone()
            .into_iter()
            .filter(|tool| {
                if tool.tool_type != AiToolType::Internal {
                    return true;
                }
                Self::should_inject_internal_tool(
                    is_coordinator_thread,
                    is_service_thread,
                    &tool.name,
                )
            })
            .collect::<Vec<_>>();
        for (name, desc, tool_type, schema) in internal_tools {
            if !Self::should_inject_internal_tool(is_coordinator_thread, is_service_thread, name) {
                continue;
            }
            if !current_tools.iter().any(|t| t.name == name) {
                current_tools.push(AiTool {
                    id: -1,
                    name: name.to_string(),
                    description: Some(desc.to_string()),
                    tool_type: AiToolType::Internal,
                    deployment_id,
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

        for tool in mcp_tools {
            if !current_tools.iter().any(|t| t.id == tool.id) {
                current_tools.push(tool);
            }
        }

        let mut agent_with_tools = ctx.agent.clone();
        agent_with_tools.tools = current_tools;
        let ctx = ctx.with_agent(agent_with_tools);

        let mut loaded_external_tool_ids: Vec<i64> = Vec::new();
        let mut virtual_tool_cache: HashMap<i64, AiTool> = HashMap::new();
        let mut task_journal_start_hash: Option<String> = None;
        let mut conversation_compaction_state = models::ConversationCompactionState::default();
        let mut pending_question: Option<models::PendingQuestion> = None;
        if let Some(state) = context.execution_state {
            loaded_external_tool_ids = state.loaded_external_tool_ids;
            for tool in state.virtual_tool_cache_snapshot {
                virtual_tool_cache.insert(tool.id, tool);
            }
            task_journal_start_hash = state.task_journal_start_hash;
            conversation_compaction_state = state.conversation_compaction_state;
            pending_question = state.pending_question;
        }

        Ok(PreparedExecutor {
            ctx,
            is_conversation_thread,
            is_coordinator_thread,
            is_review_thread,
            approved_always_tool_ids,
            project_task_board_items,
            project_task_board_id,
            system_instructions: context.system_instructions,
            loaded_external_tool_ids,
            virtual_tool_cache,
            task_journal_start_hash,
            conversation_compaction_state,
            pending_question,
            immediate_context,
        })
    }

    /// Stage B: bind the prepared state to the sandbox handle and channel. Sync — the
    /// only async work was already done in `prepare`.
    pub fn finalize(
        prepared: PreparedExecutor,
        sandbox_handle: std::sync::Arc<dyn crate::sandbox::SandboxHandle>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<AgentExecutor, AppError> {
        let PreparedExecutor {
            ctx,
            is_conversation_thread,
            is_coordinator_thread,
            is_review_thread,
            approved_always_tool_ids,
            project_task_board_items,
            project_task_board_id,
            system_instructions,
            loaded_external_tool_ids,
            virtual_tool_cache,
            task_journal_start_hash,
            conversation_compaction_state,
            pending_question,
            immediate_context,
        } = prepared;

        let tool_executor = ToolExecutor::new(ctx.clone())
            .with_channel(channel.clone())
            .with_sandbox_handle(sandbox_handle.clone());

        let filesystem = AgentFilesystem::new(
            &ctx.app_state,
            &ctx.agent.deployment_id.to_string(),
            &ctx.thread_id.to_string(),
            sandbox_handle.clone(),
        )?;

        let shell = ShellExecutor::new(sandbox_handle);

        Ok(AgentExecutor {
            ctx,
            tool_executor,
            user_request: String::new(),
            channel,
            memories: Vec::new(),
            conversations: Vec::new(),
            routing_events: Vec::new(),
            task_thread_meta: Vec::new(),
            system_instructions,
            filesystem,
            shell,
            current_iteration: 0,
            loaded_external_tool_ids,
            virtual_tool_cache,
            project_task_board_items,
            project_task_board_id,
            approved_always_tool_ids,
            task_graph_snapshot: None,
            thread_mode_cache: None,
            board_context_cache: None,
            tool_context_cache: None,
            active_thread_event: None,
            active_schedule_carryover: None,
            is_conversation_thread,
            is_coordinator_thread,
            is_review_thread,
            task_journal_start_hash,
            conversation_compaction_state,
            pending_question,
            consecutive_note_count: 0,
            last_tool_call_signature: None,
            repeated_tool_call_count: 0,
            terminal_review_continue_count: 0,
            preloaded_immediate_context: Some(immediate_context),
            budget: super::budget::BudgetCounter::default(),
        })
    }

    fn should_inject_internal_tool(
        is_coordinator_thread: bool,
        is_service_thread: bool,
        tool_name: &str,
    ) -> bool {
        if is_coordinator_thread {
            return matches!(
                tool_name,
                "list_threads"
                    | "create_thread"
                    | "update_thread"
                    | "create_project_task"
                    | "update_project_task"
                    | "assign_project_task"
                    | "read_file"
                    | "write_file"
                    | "append_file"
                    | "edit_file"
                    | "execute_command"
                    | "sleep"
            );
        }

        if is_service_thread {
            return !matches!(
                tool_name,
                "list_threads"
                    | "create_thread"
                    | "update_thread"
                    | "create_project_task"
                    | "update_project_task"
                    | "assign_project_task"
            );
        }

        true
    }
}

impl AgentExecutor {
    pub(crate) fn thread_event_implies_coordinator(event_type: &str) -> bool {
        matches!(event_type, models::thread_event::event_type::TASK_ROUTING)
    }

    /// Thread status is UI/gating metadata that only matters for conversation and
    /// coordinator threads. Service-mode runs are pure workers — writing status on
    /// them is churn. Callers pipe their `UpdateAgentThreadStateCommand` through this
    /// helper so the status field is only set when it's actually meaningful.
    pub(crate) fn apply_thread_status(
        &self,
        command: commands::UpdateAgentThreadStateCommand,
        status: models::AgentThreadStatus,
    ) -> commands::UpdateAgentThreadStateCommand {
        if self.is_conversation_thread || self.is_coordinator_thread {
            command.with_status(status)
        } else {
            command
        }
    }

    pub(crate) fn current_board_item_id(&self) -> Option<i64> {
        self.active_thread_event
            .as_ref()
            .and_then(|event| event.board_item_id)
    }

    pub(crate) fn active_task_graph_has_unfinished_nodes(&self) -> bool {
        use models::thread_task_graph::status;
        let Some(snapshot) = self.task_graph_snapshot.as_ref() else {
            return false;
        };
        if snapshot.graph.status != status::GRAPH_ACTIVE {
            return false;
        }
        snapshot.nodes.iter().any(|node| {
            node.status == status::NODE_PENDING || node.status == status::NODE_IN_PROGRESS
        })
    }

    pub(crate) fn effective_is_coordinator_thread(&self) -> bool {
        self.is_coordinator_thread
            || self
                .active_thread_event
                .as_ref()
                .map(|event| Self::thread_event_implies_coordinator(&event.event_type))
                .unwrap_or(false)
    }

    /// Resolve the role this executor is currently acting as. Mirrors
    /// `effective_is_coordinator_thread` (a routing event on a non-coordinator
    /// thread still counts as coordinator), then falls through to review /
    /// conversation / executor.
    pub(crate) fn current_thread_role(&self) -> super::project::status_machine::ThreadRole {
        use super::project::status_machine::ThreadRole;
        if self.effective_is_coordinator_thread() {
            ThreadRole::Coordinator
        } else if self.is_review_thread {
            ThreadRole::Reviewer
        } else if self.is_conversation_thread {
            ThreadRole::Conversation
        } else {
            ThreadRole::Executor
        }
    }

    pub(crate) fn system_prompt_name(&self) -> &'static str {
        if self.effective_is_coordinator_thread() {
            "coordinator_system"
        } else if self.is_review_thread {
            "reviewer_system"
        } else if self
            .active_thread_event
            .as_ref()
            .map(|e| e.event_type == models::thread_event::event_type::ASSIGNMENT_EXECUTION)
            .unwrap_or(false)
        {
            "service_execution_system"
        } else {
            "conversation_agent_system"
        }
    }
}

impl AgentExecutor {
    pub async fn new(
        ctx: std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
        sandbox_handle: std::sync::Arc<dyn crate::sandbox::SandboxHandle>,
    ) -> Result<Self, AppError> {
        let prepared = AgentExecutorBuilder::prepare(ctx, None).await?;
        AgentExecutorBuilder::finalize(prepared, sandbox_handle, channel)
    }

    pub(super) fn can_write_project_task_board_in_current_mode(&self) -> bool {
        self.effective_is_coordinator_thread()
    }

    pub(super) fn can_create_project_task_in_current_mode(&self) -> bool {
        self.effective_is_coordinator_thread() || self.is_conversation_thread
    }

    pub(super) fn tool_allowed_in_current_mode(&self, tool_name: &str) -> bool {
        match tool_name {
            "update_project_task" => self.can_write_project_task_board_in_current_mode(),
            "create_project_task" => self.can_create_project_task_in_current_mode(),
            _ => true,
        }
    }
}
