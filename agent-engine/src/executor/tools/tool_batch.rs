use super::core::AgentExecutor;
use super::tool_params::{
    PlannedToolCall, ResolvedToolCall, ToolExecutionIterationResult, ToolExecutionLoopOutcome,
};

use commands::ConsumeOnceApprovalGrantForThreadCommand;
use common::error::AppError;
use dto::json::agent_executor::{ApprovalRequestData, ToolCallRequest};
use models::{
    ActionResult, ActionResultStatus, AiTool, ConversationContent, ConversationMessageType,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
impl AgentExecutor {
    pub(crate) async fn execute_requested_actions(
        &mut self,
        requested_actions: Vec<ToolCallRequest>,
    ) -> Result<ToolExecutionLoopOutcome, AppError> {
        let planned_calls: Vec<PlannedToolCall> = requested_actions
            .into_iter()
            .map(|request| PlannedToolCall {
                request,
                retryable_on_failure: false,
            })
            .collect();
        let mut all_results = Vec::new();
        let mut prior_iteration_results: Vec<ToolExecutionIterationResult> = Vec::new();
        let mut any_pending = false;
        let mut task_graph_node_refs: HashMap<String, i64> = HashMap::new();

        if let Some(approval_request) = self
            .build_runtime_approval_request_for_calls(&planned_calls)
            .await?
        {
            self.request_user_approval(approval_request).await?;
            return Ok(ToolExecutionLoopOutcome { any_pending: true });
        }

        for call in planned_calls {
            let tool = match self.authorize_tool_call(call.tool_name()).await {
                Ok(tool) => tool,
                Err(error) => {
                    self.record_tool_execution_result(
                        &call,
                        Err(error),
                        &mut prior_iteration_results,
                        &mut all_results,
                        &mut any_pending,
                    )
                    .await?;
                    continue;
                }
            };
            let resolved = ResolvedToolCall {
                request: call,
                tool,
            };
            let execution_params =
                match Self::resolve_task_graph_request(&resolved.request, &task_graph_node_refs) {
                    Ok(parameters) => parameters,
                    Err(error) => {
                        self.record_tool_execution_result(
                            &resolved.request,
                            Err(error),
                            &mut prior_iteration_results,
                            &mut all_results,
                            &mut any_pending,
                        )
                        .await?;
                        continue;
                    }
                };

            let result = self
                .execute_planned_tool_call(&resolved, execution_params)
                .await;

            if let Ok(ref result_value) = result {
                Self::capture_task_graph_node_ref(
                    &resolved.request,
                    result_value,
                    &mut task_graph_node_refs,
                );
            }

            self.record_tool_execution_result(
                &resolved.request,
                result,
                &mut prior_iteration_results,
                &mut all_results,
                &mut any_pending,
            )
            .await?;

            if any_pending {
                break;
            }
        }

        let _ = (all_results, prior_iteration_results);
        Ok(ToolExecutionLoopOutcome { any_pending })
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

        let title = self.ctx.thread_title().await?;
        self.tool_executor
            .execute_tool_request(tool, request, &self.filesystem, &self.shell, &title)
            .await
    }

    async fn execute_planned_tool_call(
        &mut self,
        resolved: &ResolvedToolCall,
        execution_params: ToolCallRequest,
    ) -> Result<Value, AppError> {
        let _request = &resolved.request;

        match execution_params {
            ToolCallRequest::SearchTools { params, .. } => self.execute_search_tools(params).await,
            ToolCallRequest::LoadTools { params, .. } => self.execute_load_tools(params).await,
            ToolCallRequest::SnapshotExecutionState { params, .. } => {
                self.execute_snapshot_execution_state(params).await
            }
            ToolCallRequest::CreateProjectTask { params, .. } => {
                self.handle_create_project_task(params).await
            }
            ToolCallRequest::UpdateProjectTask { params, .. } => {
                self.handle_update_project_task(params).await
            }
            ToolCallRequest::AssignProjectTask { params, .. } => {
                self.handle_assign_project_task(params).await
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
        prior_iteration_results: &mut Vec<ToolExecutionIterationResult>,
        all_results: &mut Vec<ActionResult>,
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
                prior_iteration_results.push(ToolExecutionIterationResult {
                    tool_name: call.tool_name().to_string(),
                    status,
                    retryable_on_failure: call.retryable_on_failure,
                    output: Some(standardized.clone()),
                    error: None,
                });
                self.store_tool_result_conversation(
                    call,
                    prior_iteration_results
                        .last()
                        .map(|item| item.status.as_str())
                        .unwrap_or("success"),
                    Some(standardized.clone()),
                    None,
                )
                .await?;
                all_results.push(ActionResult {
                    action: call.tool_name().to_string(),
                    status: ActionResultStatus::Success,
                    result: Some(standardized),
                    error: None,
                });
            }
            Err(e) => {
                let error_message = e.to_string();
                let standardized = self.standardize_tool_output(
                    call.tool_name(),
                    None,
                    Some(error_message.clone()),
                );
                prior_iteration_results.push(ToolExecutionIterationResult {
                    tool_name: call.tool_name().to_string(),
                    status: "error".to_string(),
                    retryable_on_failure: call.retryable_on_failure,
                    output: Some(standardized.clone()),
                    error: Some(error_message.clone()),
                });
                self.store_tool_result_conversation(
                    call,
                    "error",
                    Some(standardized.clone()),
                    Some(error_message.clone()),
                )
                .await?;
                all_results.push(ActionResult {
                    action: call.tool_name().to_string(),
                    status: ActionResultStatus::Error,
                    result: Some(standardized),
                    error: Some(error_message.clone()),
                });
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
        self.store_conversation(
            ConversationContent::ToolResult {
                tool_name: call.tool_name().to_string(),
                status: status.to_string(),
                input: call.input_value()?,
                output,
                error,
            },
            ConversationMessageType::ToolResult,
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
            if !seen.insert(call.tool_name().to_string()) {
                continue;
            }

            let Some(tool) = self
                .ctx
                .agent
                .tools
                .iter()
                .find(|tool| tool.name == call.tool_name())
            else {
                continue;
            };

            if tool.requires_user_approval && !effective_approved_tool_ids.contains(&tool.id) {
                gated_tool_names.push(call.tool_name().to_string());
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
        if !tool.requires_user_approval {
            return Ok(tool);
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
}
