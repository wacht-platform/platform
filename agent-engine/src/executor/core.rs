use crate::filesystem::{shell::ShellExecutor, AgentFilesystem};
use crate::tools::ToolExecutor;

use common::error::AppError;
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
    pub(crate) ctx:
        std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
    pub(crate) conversations: Vec<ConversationRecord>,
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
    pub(crate) task_graph_snapshot: Option<serde_json::Value>,
    pub(crate) active_thread_event: Option<ThreadEvent>,
    pub(crate) is_conversation_thread: bool,
    pub(crate) is_coordinator_thread: bool,
    pub(crate) is_review_thread: bool,
    pub(crate) task_journal_start_hash: Option<String>,
    pub(crate) conversation_compaction_state: models::ConversationCompactionState,
    pub(crate) consecutive_note_count: usize,
    pub(crate) last_tool_call_signature: Option<String>,
    pub(crate) repeated_tool_call_count: usize,
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

        let knowledge_bases: Vec<(String, String)> = self
            .ctx
            .agent
            .knowledge_bases
            .iter()
            .map(|kb| (kb.id.to_string(), kb.name.clone()))
            .collect();

        let filesystem = AgentFilesystem::new(
            &self.ctx.app_state,
            &self.ctx.agent.deployment_id.to_string(),
            &self.ctx.agent.id.to_string(),
            &thread.project_id.to_string(),
            &self.ctx.thread_id.to_string(),
            &execution_id,
            knowledge_bases,
        )?;

        filesystem.spawn_initialize();

        let shell = ShellExecutor::new(filesystem.execution_root());

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
        let is_review_thread =
            context.thread_purpose == models::agent_thread::purpose::REVIEW;
        let is_service_thread = !is_coordinator_thread
            && !is_conversation_thread
            && matches!(
                context.thread_purpose.as_str(),
                models::agent_thread::purpose::EXECUTION | models::agent_thread::purpose::REVIEW
            );
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

        let mut mcp_connections = queries::GetActorMcpConnectionsQuery::new(
            self.ctx.agent.deployment_id,
            thread.actor_id,
        )
        .execute_with_db(self.ctx.app_state.db_router.writer())
        .await
        .unwrap_or_default();

        let refresh_results = futures::future::join_all(
            mcp_connections
                .iter()
                .enumerate()
                .filter(|(_, c)| crate::tools::mcp::connection_needs_refresh(c))
                .map(|(idx, conn)| async move {
                    (idx, crate::tools::mcp::refresh_connection_metadata(conn).await)
                }),
        )
        .await;

        let persist_futures = refresh_results
            .into_iter()
            .filter_map(|(idx, maybe_meta)| maybe_meta.map(|m| (idx, m)))
            .map(|(idx, new_meta)| {
                let meta_for_persist = serde_json::to_value(&new_meta).ok();
                let server_id = mcp_connections[idx].server.id;
                mcp_connections[idx].connection_metadata = Some(new_meta);
                let deployment_id = self.ctx.agent.deployment_id;
                let actor_id = thread.actor_id;
                let db = self.ctx.app_state.db_router.writer();
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

        let mcp_tools = crate::tools::mcp::discover_mcp_tools_for_actor(
            mcp_connections,
            self.ctx.agent.deployment_id,
        )
        .await;

        for tool in mcp_tools {
            if !current_tools.iter().any(|t| t.id == tool.id) {
                current_tools.push(tool);
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
            loaded_external_tool_ids: Vec::new(),
            virtual_tool_cache: std::collections::HashMap::new(),
            project_task_board_items: Vec::new(),
            project_task_board_id: None,
            approved_always_tool_ids,
            task_graph_snapshot: None,
            active_thread_event: None,
            is_conversation_thread,
            is_coordinator_thread,
            is_review_thread,
            task_journal_start_hash: None,
            conversation_compaction_state: models::ConversationCompactionState::default(),
            consecutive_note_count: 0,
            last_tool_call_signature: None,
            repeated_tool_call_count: 0,
        };

        executor.system_instructions = context.system_instructions.clone();

        if let Some(state) = context.execution_state {
            executor.restore_from_state(state)?;
        }

        if executor.is_coordinator_thread {
            executor.refresh_project_task_board_items().await?;
        }

        Ok(executor)
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
                    | "edit_file"
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
        let Some(snapshot) = self.task_graph_snapshot.as_ref() else {
            return false;
        };

        let graph_status = snapshot
            .get("graph")
            .and_then(|graph| graph.get("status"))
            .and_then(|status| status.as_str())
            .unwrap_or_default();

        if graph_status != models::thread_task_graph::status::GRAPH_ACTIVE {
            return false;
        }

        snapshot
            .get("nodes")
            .and_then(|nodes| nodes.as_array())
            .map(|nodes| {
                nodes.iter().any(|node| {
                    matches!(
                        node.get("status").and_then(|status| status.as_str()),
                        Some(
                            models::thread_task_graph::status::NODE_PENDING
                                | models::thread_task_graph::status::NODE_IN_PROGRESS
                        )
                    )
                })
            })
            .unwrap_or(false)
    }

    pub(crate) fn effective_is_coordinator_thread(&self) -> bool {
        self.is_coordinator_thread
            || self
                .active_thread_event
                .as_ref()
                .map(|event| Self::thread_event_implies_coordinator(&event.event_type))
                .unwrap_or(false)
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
    ) -> Result<Self, AppError> {
        AgentExecutorBuilder::new(ctx, channel).build().await
    }

    pub(super) fn restore_from_state(
        &mut self,
        state: ThreadExecutionState,
    ) -> Result<(), AppError> {
        self.loaded_external_tool_ids = state.loaded_external_tool_ids;
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

    pub(super) fn tool_allowed_in_current_mode(&self, tool_name: &str) -> bool {
        match tool_name {
            "update_project_task" => self.can_write_project_task_board_in_current_mode(),
            "create_project_task" => self.can_create_project_task_in_current_mode(),
            _ => true,
        }
    }
}
