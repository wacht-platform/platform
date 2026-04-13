use super::core::AgentExecutor;
use crate::llm::{SemanticLlmPromptConfig, SemanticLlmRequest};
use crate::template::{render_prompt_text, render_template_json, AgentTemplates};

use common::error::AppError;
use dto::json::agent_executor::{
    ExecuteActionDirective, ToolCallBrief, ToolCallRequest, ToolExecutionPlan,
};
use dto::json::{
    LlmHistoryEntry, ProjectTaskBoardAssignmentPromptItem, ProjectTaskBoardItemEventPromptItem,
};
use models::AiTool;
use queries::{
    GetProjectTaskBoardItemAssignmentByIdQuery, ListAssignmentsForThreadQuery,
    ListProjectTaskBoardItemAssignmentsQuery, ListProjectTaskBoardItemEventsQuery,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub(crate) const MAX_LOADED_EXTERNAL_TOOLS: usize = 10;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PlannedToolCall {
    pub(crate) request: ToolCallRequest,
    pub(crate) retryable_on_failure: bool,
}

impl PlannedToolCall {
    pub(crate) fn tool_name(&self) -> &str {
        self.request.tool_name()
    }

    pub(crate) fn input_value(&self) -> Result<Value, AppError> {
        self.request
            .input_value()
            .map_err(|e| AppError::Internal(format!("Failed to serialize tool input: {e}")))
    }
}

#[derive(Clone)]
pub(crate) struct ResolvedToolCall {
    pub(crate) request: PlannedToolCall,
    pub(crate) tool: AiTool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ToolExecutionIterationResult {
    pub tool_name: String,
    pub status: String,
    pub retryable_on_failure: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub(crate) struct ToolExecutionLoopOutcome {
    pub any_pending: bool,
}
impl AgentExecutor {
    pub(crate) fn normalize_tool_execution_plan(
        &self,
        plan: ToolExecutionPlan,
    ) -> ToolExecutionPlan {
        plan
    }

    pub(crate) fn validate_tool_execution_plan(
        &self,
        directive: &ExecuteActionDirective,
        plan: &ToolExecutionPlan,
    ) -> Result<(), AppError> {
        let allowed = directive
            .allowed_tools
            .iter()
            .map(|tool| tool.trim())
            .filter(|tool| !tool.is_empty())
            .collect::<std::collections::HashSet<_>>();

        let invalid = plan
            .tool_calls
            .iter()
            .map(|call| call.tool_name())
            .filter(|tool| !allowed.contains(tool))
            .collect::<Vec<_>>();

        if invalid.is_empty() {
            return Ok(());
        }

        Err(AppError::BadRequest(format!(
            "Invalid tool execution plan: tool calls must stay inside the allowed tool set. Invalid entries: {}",
            invalid.join(", ")
        )))
    }

    fn summarize_execution_environment(
        thread_title: &str,
        thread_purpose: &str,
        thread_responsibility: &str,
        accepts_assignments: bool,
        reusable: bool,
        is_coordinator_thread: bool,
        execution_mode: &str,
    ) -> String {
        format!(
            "- title: {thread_title}\n- purpose: {thread_purpose}\n- responsibility: {thread_responsibility}\n- execution_mode: {execution_mode}\n- accepts_assignments: {accepts_assignments}\n- reusable: {reusable}\n- coordinator_thread: {is_coordinator_thread}"
        )
    }

    pub(crate) fn summarize_active_assignment(
        assignment: &ProjectTaskBoardAssignmentPromptItem,
    ) -> Option<String> {
        let mode = assignment.mode.as_deref().unwrap_or("unknown");
        let assignment_role = assignment.assignment_role.as_str();
        let assignment_order = assignment.assignment_order.to_string();
        let status = assignment.status.as_str();
        let thread_id = assignment.thread_id.to_string();
        let instructions = assignment
            .instructions
            .as_deref()
            .map(str::trim)
            .map(ToString::to_string)
            .unwrap_or_default();
        let handoff = assignment.handoff_file_path.as_deref().unwrap_or("");
        let result_summary = assignment
            .result_summary
            .as_deref()
            .map(str::trim)
            .map(ToString::to_string)
            .unwrap_or_default();

        Some(format!(
            "- mode: {mode}\n- role/order/status: {assignment_role} #{assignment_order} [{status}]\n- thread_id: {thread_id}\n- instructions: {}\n- handoff_file_path: {}\n- result_summary: {}",
            if instructions.is_empty() { "(none)" } else { &instructions },
            if handoff.is_empty() { "(none)" } else { handoff },
            if result_summary.is_empty() { "(none)" } else { &result_summary }
        ))
    }

    pub(crate) fn summarize_assignment_list(
        assignments: &[ProjectTaskBoardAssignmentPromptItem],
        limit: usize,
    ) -> Option<String> {
        if assignments.is_empty() {
            return None;
        }

        let mut lines = Vec::new();
        for assignment in assignments.iter().take(limit) {
            let role = assignment.assignment_role.as_str();
            let order = assignment.assignment_order.to_string();
            let status = assignment.status.as_str();
            let thread_id = assignment.thread_id.to_string();
            let handoff = assignment.handoff_file_path.as_deref().unwrap_or("");
            let instructions = assignment
                .instructions
                .as_deref()
                .map(str::trim)
                .map(ToString::to_string)
                .unwrap_or_default();

            let mut line = format!("- {role} #{order} on thread {thread_id} [{status}]");
            if !handoff.is_empty() {
                line.push_str(&format!(" handoff={handoff}"));
            }
            if !instructions.is_empty() {
                line.push_str(&format!(" instructions=\"{instructions}\""));
            }
            lines.push(line);
        }

        if assignments.len() > limit {
            lines.push(format!(
                "- ... {} more assignment(s) omitted",
                assignments.len() - limit
            ));
        }

        Some(lines.join("\n"))
    }

    fn summarize_board_events(
        events: &[ProjectTaskBoardItemEventPromptItem],
        limit: usize,
    ) -> Option<String> {
        if events.is_empty() {
            return None;
        }

        let start = events.len().saturating_sub(limit);
        let mut lines = Vec::new();

        for event in events.iter().skip(start) {
            let created_at = event.created_at.as_str();
            let event_type = event.event_type.as_str();
            let summary = event.summary.trim().to_string();

            let mut line = format!("- [{created_at}] {event_type}");
            if !summary.is_empty() {
                line.push_str(&format!(": {summary}"));
            }
            lines.push(line);
        }

        if start > 0 {
            lines.insert(0, format!("- ... {} older event(s) omitted", start));
        }

        Some(lines.join("\n"))
    }

    fn summarize_thread_assignment_queue(
        queue: &[ProjectTaskBoardAssignmentPromptItem],
        limit: usize,
    ) -> Option<String> {
        if queue.is_empty() {
            return None;
        }

        let mut lines = vec![format!("- pending workload count: {}", queue.len())];
        for assignment in queue.iter().take(limit) {
            let role = assignment.assignment_role.as_str();
            let order = assignment.assignment_order.to_string();
            let status = assignment.status.as_str();
            let board_item_id = assignment.board_item_id.to_string();
            lines.push(format!(
                "- board_item {board_item_id}: {role} #{order} [{status}]"
            ));
        }

        if queue.len() > limit {
            lines.push(format!(
                "- ... {} more queued assignment(s) omitted",
                queue.len() - limit
            ));
        }

        Some(lines.join("\n"))
    }

    fn summarize_task_graph(task_graph: &Value, node_limit: usize) -> Option<String> {
        let graph = task_graph.get("graph")?;
        let graph_id = graph
            .get("id")
            .and_then(|v| {
                v.as_i64()
                    .map(|v| v.to_string())
                    .or_else(|| v.as_str().map(|s| s.to_string()))
            })
            .unwrap_or_else(|| "unknown".to_string());
        let status = graph
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let mut lines = vec![format!("- graph_id: {graph_id}")];
        lines.push(format!("- graph_status: {status}"));

        if let Some(nodes) = task_graph.get("nodes").and_then(|v| v.as_array()) {
            lines.push(format!("- node_count: {}", nodes.len()));
            for node in nodes.iter().take(node_limit) {
                let node_id = node
                    .get("id")
                    .and_then(|v| {
                        v.as_i64()
                            .map(|v| v.to_string())
                            .or_else(|| v.as_str().map(|s| s.to_string()))
                    })
                    .unwrap_or_else(|| "unknown".to_string());
                let title = node
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("untitled");
                let node_status = node
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                lines.push(format!(
                    "- node {node_id}: {} [{}]",
                    title.trim(),
                    node_status
                ));
            }
            if nodes.len() > node_limit {
                lines.push(format!(
                    "- ... {} more node(s) omitted",
                    nodes.len() - node_limit
                ));
            }
        }

        Some(lines.join("\n"))
    }

    async fn current_active_assignment_value(
        &self,
    ) -> Option<ProjectTaskBoardAssignmentPromptItem> {
        let (mode, assignment_id) = self.active_thread_event.as_ref().and_then(|event| {
            event
                .assignment_execution_payload()
                .map(|payload| (Some("assignment_execution"), payload.assignment_id))
                .or_else(|| {
                    event
                        .assignment_outcome_review_payload()
                        .map(|payload| (Some("assignment_outcome_review"), payload.assignment_id))
                })
        })?;

        let assignment = GetProjectTaskBoardItemAssignmentByIdQuery::new(assignment_id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
            .ok()
            .flatten()?;

        let mut item = Self::assignment_prompt_item_from_row(&assignment);
        item.mode = mode.map(ToString::to_string);
        Some(item)
    }

    pub(crate) async fn execute_action_iteration(
        &mut self,
        directive: &ExecuteActionDirective,
    ) -> Result<ToolExecutionLoopOutcome, AppError> {
        let brief = Self::normalized_tool_call_brief(
            directive.tool_call_brief.clone().unwrap_or_default(),
            directive.objective.trim(),
        );
        self.execute_action_iteration_with_brief(directive, &brief)
            .await
    }

    async fn execute_action_iteration_with_brief(
        &mut self,
        directive: &ExecuteActionDirective,
        brief: &ToolCallBrief,
    ) -> Result<ToolExecutionLoopOutcome, AppError> {
        let plan = self.plan_tool_execution_iteration(directive, brief).await?;

        if plan.tool_calls.is_empty() {
            return Ok(ToolExecutionLoopOutcome { any_pending: false });
        }

        self.execute_requested_actions(plan.tool_calls).await
    }

    pub(crate) async fn execute_action_iteration_with_guidance(
        &mut self,
        directive: &ExecuteActionDirective,
        guidance: Option<&str>,
    ) -> Result<ToolExecutionLoopOutcome, AppError> {
        let mut guided_plan = Self::normalized_tool_call_brief(
            directive.tool_call_brief.clone().unwrap_or_default(),
            directive.objective.trim(),
        );
        if let Some(guidance) = guidance.map(str::trim).filter(|g| !g.is_empty()) {
            guided_plan
                .constraints
                .insert(0, format!("Continue-action guidance: {guidance}"));
        }
        self.execute_action_iteration_with_brief(directive, &guided_plan)
            .await
    }

    fn normalized_tool_call_brief(mut brief: ToolCallBrief, objective: &str) -> ToolCallBrief {
        brief.focus_points = brief
            .focus_points
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .take(6)
            .collect();
        brief.tool_parameter_briefs = brief
            .tool_parameter_briefs
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .take(6)
            .collect();
        brief.constraints = brief
            .constraints
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .take(8)
            .collect();
        if brief.focus_points.is_empty() {
            brief.focus_points.push(objective.to_string());
        }
        brief
    }

    async fn plan_tool_execution_iteration(
        &mut self,
        directive: &ExecuteActionDirective,
        tool_call_brief: &ToolCallBrief,
    ) -> Result<ToolExecutionPlan, AppError> {
        let active_board_item: Option<dto::json::ProjectTaskBoardPromptItem> =
            self.active_board_item_prompt_item().await?;
        let available_tools = self
            .available_tools_for_mode()
            .await
            .into_iter()
            .filter(|tool| !matches!(tool.name.as_str(), "search_tools" | "load_tools"))
            .filter(|tool| {
                directive
                    .allowed_tools
                    .iter()
                    .any(|name| name == &tool.name)
            })
            .collect::<Vec<_>>();
        let task_graph = self.ensure_task_graph_snapshot().await?;
        let flat_tool_selection_properties =
            self.build_flat_tool_selection_properties(&available_tools, active_board_item.as_ref());

        let thread = self.ctx.get_thread().await?;
        let thread_purpose = thread.thread_purpose.clone();
        let active_board_item_summary = active_board_item
            .as_ref()
            .map(|item| {
                format!(
                    "Active board item:\n- task_key: {}\n- title: {}\n- status: {}\n- priority: {}\n- description: {}",
                    item.task_key,
                    item.title,
                    item.status,
                    item.priority,
                    item.description.clone().unwrap_or_default()
                )
            })
            .unwrap_or_else(|| "No active board item.".to_string());
        let triggering_event_type = self
            .active_thread_event
            .as_ref()
            .map(|event| event.event_type.clone());
        let execution_mode = match triggering_event_type.as_deref() {
            Some(models::thread_event::event_type::TASK_ROUTING) => "task_routing",
            Some(models::thread_event::event_type::ASSIGNMENT_EXECUTION) => "assignment_execution",
            Some(models::thread_event::event_type::ASSIGNMENT_OUTCOME_REVIEW) => {
                "assignment_outcome_review"
            }
            _ if self.is_coordinator_thread => "coordinator",
            _ => "normal",
        };
        let active_assignment = self.current_active_assignment_value().await;
        let active_board_item_id = active_assignment
            .as_ref()
            .map(|assignment| assignment.board_item_id)
            .or_else(|| {
                self.active_thread_event
                    .as_ref()
                    .and_then(|event| event.board_item_id)
            });
        let active_board_item_assignments = if let Some(item_id) = active_board_item_id {
            ListProjectTaskBoardItemAssignmentsQuery::new(item_id)
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|assignment| Self::assignment_prompt_item_from_row(&assignment))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let active_board_item_events = if let Some(item_id) = active_board_item_id {
            let mut events = ListProjectTaskBoardItemEventsQuery::new(item_id)
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await
                .unwrap_or_default()
                .into_iter()
                .take(10)
                .map(Self::board_item_event_prompt_item)
                .collect::<Vec<_>>();
            events.reverse();
            events
        } else {
            Vec::new()
        };
        let thread_assignment_queue = ListAssignmentsForThreadQuery::new(self.ctx.thread_id)
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
            .collect::<Vec<_>>();
        let execution_environment_summary = Self::summarize_execution_environment(
            &thread.title,
            &thread_purpose,
            thread.responsibility.as_deref().unwrap_or("Unspecified"),
            thread.accepts_assignments,
            thread.reusable,
            self.is_coordinator_thread,
            execution_mode,
        );
        let active_assignment_summary = active_assignment
            .as_ref()
            .and_then(Self::summarize_active_assignment);
        let active_board_item_assignments_summary =
            Self::summarize_assignment_list(&active_board_item_assignments, 6);
        let recent_assignment_history = if active_board_item_assignments.len() > 5 {
            active_board_item_assignments[active_board_item_assignments.len() - 5..].to_vec()
        } else {
            active_board_item_assignments.clone()
        };
        let recent_assignment_history_summary =
            Self::summarize_assignment_list(&recent_assignment_history, 5);
        let active_board_item_events_summary =
            Self::summarize_board_events(&active_board_item_events, 6);
        let thread_assignment_queue_summary =
            Self::summarize_thread_assignment_queue(&thread_assignment_queue, 4);
        let task_graph_summary = Self::summarize_task_graph(&task_graph, 6);
        let conversation_history_prefix = self.get_conversation_history_for_llm().await;
        let task_journal_tail = if active_board_item_id.is_some() {
            self.task_journal_tail_snippet().await?
        } else {
            None
        };

        let execution_request_message = format!(
            "Parent next-step decision already selected startaction for this thread. Stay inside that branch only.\n\nUse the recent conversation history for the active startaction objective and any continueaction guidance. Do not restate or broaden the action contract.\n\nSelected tool calls execute serially in the order emitted across the batch. If a later step depends on an earlier side effect or output, emit separate ordered tool calls instead of shell chaining like `&&`.\n\nTool call brief:\n- focus_points:\n{}\n- tool_parameter_briefs:\n{}\n- constraints:\n{}\n\nSelect only the next immediate runnable batch. If a previous tool call failed, self-correct from the live action-execution context instead of repeating it unchanged.",
            if tool_call_brief.focus_points.is_empty() {
                "  - (none)".to_string()
            } else {
                tool_call_brief
                    .focus_points
                    .iter()
                    .map(|item| format!("  - {item}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            },
            if tool_call_brief.tool_parameter_briefs.is_empty() {
                "  - (none)".to_string()
            } else {
                tool_call_brief
                    .tool_parameter_briefs
                    .iter()
                    .map(|item| format!("  - {item}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            },
            if tool_call_brief.constraints.is_empty() {
                "  - (none)".to_string()
            } else {
                tool_call_brief
                    .constraints
                    .iter()
                    .map(|item| format!("  - {item}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        );

        let current_request_entry =
            conversation_history_prefix
                .last()
                .cloned()
                .unwrap_or(LlmHistoryEntry::with_parts(
                    "user",
                    "user_message",
                    None,
                    vec![dto::json::LlmHistoryPart::text(
                "[No explicit current request message. Stay inside the selected startaction.]",
            )],
                ));

        let live_context_message = format!(
            r#"LIVE ACTION EXECUTION CONTEXT
Treat this as the current dynamic state for this execution pass.

## Execution Environment
{}

## Active Assignment
{}

## Recent Assignment History
{}

## Latest Journal State
{}

## Active Board Item
{}

## Active Board Item Assignments
{}

## Recent Board Item Events
{}

## Thread Assignment Queue
{}

## Execution Task Graph
{}"#,
            execution_environment_summary,
            active_assignment_summary
                .clone()
                .unwrap_or_else(|| "none".to_string()),
            recent_assignment_history_summary
                .clone()
                .unwrap_or_else(|| "none".to_string()),
            task_journal_tail
                .clone()
                .unwrap_or_else(|| "none".to_string()),
            active_board_item_summary,
            active_board_item_assignments_summary
                .clone()
                .unwrap_or_else(|| "none".to_string()),
            active_board_item_events_summary
                .clone()
                .unwrap_or_else(|| "none".to_string()),
            thread_assignment_queue_summary
                .clone()
                .unwrap_or_else(|| "none".to_string()),
            task_graph_summary
                .clone()
                .unwrap_or_else(|| "none".to_string())
        );

        let mut template_context = json!({
            "has_conversation_history_prefix": !conversation_history_prefix.is_empty(),
            "conversation_history_prefix": conversation_history_prefix,
            "current_request_entry": current_request_entry,
            "live_context_message": live_context_message,
            "action_execution_mode": true,
            "action_execution_request_message": execution_request_message,
            "response_json_schema": json!({ "type": "OBJECT", "properties": flat_tool_selection_properties }),
        });

        if let Some(obj) = template_context.as_object_mut() {
            obj.insert(
                "execution_environment_summary".to_string(),
                json!(execution_environment_summary),
            );
            obj.insert(
                "active_assignment_summary".to_string(),
                json!(active_assignment_summary),
            );
            obj.insert(
                "active_board_item_summary".to_string(),
                json!(active_board_item_summary),
            );
            obj.insert(
                "active_board_item_assignments_summary".to_string(),
                json!(active_board_item_assignments_summary),
            );
            obj.insert(
                "recent_assignment_history_summary".to_string(),
                json!(recent_assignment_history_summary),
            );
            obj.insert("task_journal_tail".to_string(), json!(task_journal_tail));
            obj.insert(
                "active_board_item_events_summary".to_string(),
                json!(active_board_item_events_summary),
            );
            obj.insert(
                "thread_assignment_queue_summary".to_string(),
                json!(thread_assignment_queue_summary),
            );
            obj.insert("task_graph_summary".to_string(), json!(task_graph_summary));
        }

        let config: SemanticLlmPromptConfig =
            render_template_json(AgentTemplates::STEP_DECISION, &template_context)?;
        let system_prompt = render_prompt_text("next_step_decision_system", &template_context)?;
        let messages = self.build_next_step_decision_messages(
            &conversation_history_prefix,
            &live_context_message,
            &current_request_entry,
            Some(&execution_request_message),
        );
        let request = SemanticLlmRequest::from_config(system_prompt, messages, config);
        let native_tools = available_tools
            .iter()
            .map(|tool| self.build_native_tool_definition(tool, active_board_item.as_ref()))
            .collect::<Vec<_>>();

        let llm = self.create_weak_llm().await?;
        let output = llm.generate_tool_calls(request, native_tools).await?;
        self.record_llm_usage_for_compaction(output.usage_metadata.as_ref());
        let tool_calls = output
            .calls
            .into_iter()
            .map(|call| {
                let tool = available_tools
                    .iter()
                    .find(|tool| tool.name == call.tool_name)
                    .ok_or_else(|| {
                        AppError::BadRequest(format!(
                            "Selected tool '{}' is not available in the current execution mode",
                            call.tool_name
                        ))
                    })?;
                let input_object = call.arguments.as_object().ok_or_else(|| {
                    AppError::BadRequest(format!(
                        "Selected tool '{}' must use JSON object arguments",
                        call.tool_name
                    ))
                })?;
                self.build_tool_call_request_from_native_call(tool, input_object.clone())
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        let plan = ToolExecutionPlan { tool_calls };
        let plan = self.normalize_tool_execution_plan(plan);
        self.validate_tool_execution_plan(directive, &plan)?;
        Ok(plan)
    }
}
