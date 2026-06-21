use super::core::AgentExecutor;
use super::tool_params::{PlannedToolCall, ResolvedToolCall, ToolExecutionLoopOutcome};

use crate::tools::approval::resolve_approval_action;
use chrono::Utc;
use commands::ConsumeOnceApprovalGrantForThreadCommand;
use common::error::AppError;
use dto::json::agent_executor::{ApprovalRequestData, ToolCallRequest};
use models::{AiTool, ApprovalAction, ConversationContent, ConversationMessageType};
use serde_json::Value;
use std::collections::HashSet;
use std::time::Duration;

const MAX_TOOL_ERROR_INPUT_CHARS: usize = 300;
const TOOL_CALL_TIMEOUT: Duration = Duration::from_secs(900);

fn truncate_chars(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("…[truncated]");
    out
}

fn input_preview(input: &Value) -> String {
    let serialized = serde_json::to_string(input).unwrap_or_else(|_| "<unrenderable>".into());
    truncate_chars(&serialized, MAX_TOOL_ERROR_INPUT_CHARS)
}

fn audit_line(
    iteration: usize,
    tool: &str,
    status: &str,
    input: &str,
    error: Option<&str>,
) -> String {
    let error_part = error
        .map(|e| format!(" error=\"{}\"", truncate_chars(&e.replace('\n', " "), 400)))
        .unwrap_or_default();
    format!(
        "[{}] iter={iteration} tool={tool} status={status} input={input}{error_part}",
        Utc::now().to_rfc3339()
    )
}
impl AgentExecutor {
    #[tracing::instrument(
        name = "tools.execute_batch",
        skip(self, requested_actions),
        fields(
            thread_id = self.ctx.thread_id,
            board_item_id = ?self.current_board_item_id(),
            execution_run_id = self.ctx.execution_run_id,
            batch_size = requested_actions.len(),
        )
    )]
    pub(crate) async fn execute_requested_actions(
        &mut self,
        requested_actions: Vec<(ToolCallRequest, Option<String>)>,
        origin_provider: String,
        origin_model: String,
    ) -> Result<ToolExecutionLoopOutcome, AppError> {
        let planned_calls: Vec<PlannedToolCall> = requested_actions
            .into_iter()
            .map(|(request, signature)| PlannedToolCall {
                request,
                retryable_on_failure: false,
                signature,
                origin_provider: origin_provider.clone(),
                origin_model: origin_model.clone(),
            })
            .collect();
        let mut any_pending = false;

        if let Some(approval_request) = self
            .build_runtime_approval_request_for_calls(&planned_calls)
            .await?
        {
            self.request_user_approval(approval_request).await?;
            return Ok(ToolExecutionLoopOutcome { any_pending: true });
        }

        let batch_was_empty = planned_calls.is_empty();
        let mut audit_lines: Vec<String> = Vec::new();
        let mut failed_tools: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        let mut succeeded_tools: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();

        for call in planned_calls {
            let tool_name = call.tool_name().to_string();
            let preview = call
                .input_value()
                .ok()
                .map(|v| input_preview(&v))
                .unwrap_or_else(|| "<unavailable>".to_string());

            let tool = match self.authorize_tool_call(&tool_name).await {
                Ok(tool) => tool,
                Err(error) => {
                    audit_lines.push(audit_line(
                        self.current_iteration,
                        &tool_name,
                        "error",
                        &preview,
                        Some(&error.to_string()),
                    ));
                    failed_tools.insert(tool_name.clone());
                    self.record_tool_execution_result(&call, Err(error), &mut any_pending)
                        .await?;
                    continue;
                }
            };
            let resolved = ResolvedToolCall {
                request: call,
                tool,
            };

            let result = match tokio::time::timeout(
                TOOL_CALL_TIMEOUT,
                self.execute_planned_tool_call(&resolved),
            )
            .await
            {
                Ok(r) => r,
                Err(_) => Err(AppError::Internal(format!(
                    "tool '{tool_name}' timed out after {}s",
                    TOOL_CALL_TIMEOUT.as_secs()
                ))),
            };

            if Self::tool_name_mutates_task_graph(&tool_name) && result.is_ok() {
                self.invalidate_task_graph_snapshot();
            }

            let call_failed = match &result {
                Err(_) => true,
                Ok(value) => value.get("success").and_then(|s| s.as_bool()) == Some(false),
            };
            let (audit_status, audit_error) = match &result {
                Err(error) => ("error", Some(error.to_string())),
                Ok(value) if call_failed => (
                    "failed",
                    value
                        .get("error")
                        .map(|e| e.to_string())
                        .filter(|e| e != "null"),
                ),
                Ok(_) => ("success", None),
            };
            audit_lines.push(audit_line(
                self.current_iteration,
                &tool_name,
                audit_status,
                &preview,
                audit_error.as_deref(),
            ));
            if call_failed {
                failed_tools.insert(tool_name.clone());
            } else {
                succeeded_tools.insert(tool_name.clone());
            }

            self.record_tool_execution_result(&resolved.request, result, &mut any_pending)
                .await?;

            if any_pending {
                break;
            }
        }

        if !batch_was_empty {
            self.update_consecutive_tool_failures(&failed_tools, &succeeded_tools);
        }
        self.append_task_audit(&audit_lines).await;

        Ok(ToolExecutionLoopOutcome { any_pending })
    }

    // Same-tool failure streak; consumed only by reasoning-effort escalation.
    fn update_consecutive_tool_failures(
        &mut self,
        failed: &std::collections::BTreeSet<String>,
        succeeded: &std::collections::BTreeSet<String>,
    ) {
        if let Some(tracked) = self.last_failed_tool_label.clone() {
            if succeeded.contains(&tracked) {
                self.last_failed_tool_label = None;
                self.consecutive_tool_failure_count = 0;
                return;
            }
            if failed.contains(&tracked) {
                self.consecutive_tool_failure_count =
                    self.consecutive_tool_failure_count.saturating_add(1);
                return;
            }
        }

        match failed.iter().next() {
            Some(first_failed) => {
                self.last_failed_tool_label = Some(first_failed.clone());
                self.consecutive_tool_failure_count = 1;
            }
            None => {
                self.last_failed_tool_label = None;
                self.consecutive_tool_failure_count = 0;
            }
        }
    }

    pub(in crate::executor) async fn audit_rejected_call(
        &mut self,
        tool_name: &str,
        arguments: &Value,
        error: &str,
    ) {
        let line = audit_line(
            self.current_iteration,
            tool_name,
            "rejected",
            &input_preview(arguments),
            Some(error),
        );
        self.append_task_audit(&[line]).await;
    }

    async fn append_task_audit(&mut self, lines: &[String]) {
        // Every task lane (coordinator/executor/reviewer/delegated) keeps its own
        // tool-call log under /task/audit/ for per-lane evaluation. Conversation
        // turns have no board-item task workspace, so they're skipped.
        if lines.is_empty() || self.current_board_item_id().is_none() {
            return;
        }
        let role = if self.is_delegated_task {
            "delegated"
        } else {
            self.current_thread_role().as_str()
        };
        let audit_path = format!(
            "{}/{}-{}.log",
            crate::runtime::task_workspace::TASK_WORKSPACE_AUDIT_DIR,
            role,
            self.ctx.thread_id,
        );
        let mut content = String::new();
        if !self.audit_run_header_written {
            self.audit_run_header_written = true;
            content.push_str(&format!(
                "[execution run={} thread={} role={} assignment={} started={}]\n",
                self.ctx.execution_run_id,
                self.ctx.thread_id,
                role,
                self.current_assignment_id()
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                Utc::now().to_rfc3339(),
            ));
        }
        content.push_str(&lines.join("\n"));
        if let Err(error) = self
            .filesystem
            .write_file(&audit_path, &content, true)
            .await
        {
            tracing::warn!(
                thread_id = self.ctx.thread_id,
                execution_run_id = self.ctx.execution_run_id,
                ?error,
                "task audit append failed"
            );
        }
    }

    async fn execute_tool_call_direct(
        &self,
        tool: &AiTool,
        request: &ToolCallRequest,
    ) -> Result<Value, AppError> {
        let tool_name = request.tool_name();

        if !self.tool_allowed_in_current_mode(tool_name) {
            return Err(AppError::BadRequest(format!(
                "Tool '{}' is not available in the current execution mode",
                tool_name
            )));
        }

        self.tool_executor
            .execute_tool_request(tool, request, &self.filesystem, &self.shell)
            .await
    }

    async fn execute_planned_tool_call(
        &mut self,
        resolved: &ResolvedToolCall,
    ) -> Result<Value, AppError> {
        match resolved.request.request.clone() {
            ToolCallRequest::SearchTools { params, .. } => self.execute_search_tools(params).await,
            ToolCallRequest::LoadTools { params, .. } => self.execute_load_tools(params).await,
            ToolCallRequest::CreateProjectTask { params, .. } => {
                self.handle_create_project_task(params).await
            }
            ToolCallRequest::UpdateProjectTask { params, .. } => {
                self.handle_update_project_task(params).await
            }
            ToolCallRequest::AssignProjectTask { params, .. } => {
                self.handle_assign_project_task(params).await
            }
            ToolCallRequest::SubscribeToTask { params, .. } => {
                self.handle_subscribe_to_task(params).await
            }
            ToolCallRequest::UnsubscribeFromTask { params, .. } => {
                self.handle_unsubscribe_from_task(params).await
            }
            ToolCallRequest::GetProjectTask { params, .. } => {
                self.handle_get_project_task(params).await
            }
            request => {
                self.execute_tool_call_direct(&resolved.tool, &request)
                    .await
            }
        }
    }

    async fn record_tool_execution_result(
        &mut self,
        call: &PlannedToolCall,
        result: Result<Value, AppError>,
        any_pending: &mut bool,
    ) -> Result<(), AppError> {
        match result {
            Ok(result_value) => {
                let standardized =
                    self.standardize_tool_output(call.tool_name(), Some(&result_value), None);
                let status = standardized
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("success")
                    .to_string();
                if status == "pending" {
                    *any_pending = true;
                }
                self.store_tool_result_conversation(call, &status, Some(standardized), None)
                    .await?;
            }
            Err(e) => {
                let error_message = e.to_string();
                let standardized = self.standardize_tool_output(
                    call.tool_name(),
                    None,
                    Some(error_message.clone()),
                );
                self.store_tool_result_conversation(
                    call,
                    "error",
                    Some(standardized),
                    Some(error_message),
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn store_tool_result_conversation(
        &mut self,
        call: &PlannedToolCall,
        status: &str,
        output: Option<Value>,
        error: Option<String>,
    ) -> Result<(), AppError> {
        let metadata = serde_json::json!({
            "signature": call.signature.clone(),
            "origin_provider": call.origin_provider.clone(),
            "origin_model": call.origin_model.clone(),
        });
        self.store_conversation_with_metadata(
            ConversationContent::ToolResult {
                tool_name: call.tool_name().to_string(),
                status: status.to_string(),
                input: call.input_value()?,
                output,
                error,
            },
            ConversationMessageType::ToolResult,
            Some(metadata),
        )
        .await
    }

    async fn build_runtime_approval_request_for_calls(
        &self,
        planned_calls: &[PlannedToolCall],
    ) -> Result<Option<ApprovalRequestData>, AppError> {
        let effective_approved_tool_ids = self.effective_approved_tool_ids().await?;
        let mut seen = HashSet::new();
        let mut gated_tool_names = Vec::new();

        for call in planned_calls {
            let tool_name = call.tool_name().to_string();
            if !seen.insert(tool_name.clone()) {
                continue;
            }

            if !matches!(
                resolve_approval_action(&self.ctx.agent, &tool_name),
                ApprovalAction::Review
            ) {
                continue;
            }

            let already_approved = self
                .ctx
                .agent
                .tools
                .iter()
                .find(|tool| tool.name == tool_name)
                .map(|tool| effective_approved_tool_ids.contains(&tool.id))
                .unwrap_or(false);
            if !already_approved {
                gated_tool_names.push(tool_name);
            }
        }

        if gated_tool_names.is_empty() {
            return Ok(None);
        }

        Ok(Some(ApprovalRequestData {
            description: format!(
                "Approval required to run the current tool batch: {}.",
                gated_tool_names.join(", ")
            ),
            tool_names: gated_tool_names,
        }))
    }

    async fn find_available_tool(&self, tool_name: &str) -> Result<AiTool, AppError> {
        self.available_tools_for_mode()
            .await
            .into_iter()
            .find(|t| t.name == tool_name)
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "Tool '{tool_name}' is not available in the current execution mode"
                ))
            })
    }

    async fn authorize_tool_call(&mut self, tool_name: &str) -> Result<AiTool, AppError> {
        let tool = self.find_available_tool(tool_name).await?;
        match resolve_approval_action(&self.ctx.agent, tool_name) {
            ApprovalAction::Allow => return Ok(tool),
            ApprovalAction::Deny => {
                return Err(AppError::BadRequest(format!(
                    "Tool '{tool_name}' denied by agent approval policy"
                )));
            }
            ApprovalAction::Review => {}
        }

        self.refresh_inherited_approval_state().await?;

        if self.approved_always_tool_ids.contains(&tool.id) {
            return Ok(tool);
        }

        let consumed_once_approval = ConsumeOnceApprovalGrantForThreadCommand::new(
            self.ctx.agent.deployment_id,
            self.ctx.thread_id,
            tool.id,
        )
        .execute_with_db(self.ctx.app_state.db_router.writer())
        .await?;

        if consumed_once_approval.is_some() {
            return Ok(tool);
        }

        Err(AppError::BadRequest(format!(
            "Tool '{}' requires user approval and no active grant is available.",
            tool_name
        )))
    }

    async fn refresh_inherited_approval_state(&mut self) -> Result<(), AppError> {
        let active_approvals = queries::ListActiveApprovalGrantsForThreadQuery::new(
            self.ctx.agent.deployment_id,
            self.ctx.thread_id,
        )
        .execute_with_db(self.ctx.app_state.db_router.writer())
        .await
        .unwrap_or_default();

        for approval in active_approvals {
            if approval.grant_scope != models::approval::grant_scope::ONCE {
                self.approved_always_tool_ids.insert(approval.tool_id);
            }
        }

        Ok(())
    }

    fn tool_name_mutates_task_graph(tool_name: &str) -> bool {
        matches!(
            tool_name,
            "task_graph_add_node"
                | "task_graph_add_dependency"
                | "task_graph_mark_in_progress"
                | "task_graph_complete_node"
                | "task_graph_fail_node"
                | "task_graph_reset"
        )
    }
}
