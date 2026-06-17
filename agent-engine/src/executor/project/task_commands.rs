use super::core::AgentExecutor;
use common::ResultExt;

use crate::llm::{SemanticLlmMessage, SemanticLlmRequest};
use crate::runtime::task_workspace::{
    append_journal_handoff_entry, finalize_journal_compaction, prepare_journal_compaction,
    render_rule_only_activity, CheckpointInputs, HandoffPayload, TASK_WORKSPACE_JOURNAL_FILE,
};
use common::error::AppError;
use dto::json::agent_executor::{
    AssignProjectTaskParams, CreateProjectTaskParams, GetProjectTaskParams, SubscribeToTaskParams,
    UnsubscribeFromTaskParams, UpdateProjectTaskParams,
};
use dto::json::ProjectTaskScheduleParams;
use models::{ProjectTaskBoardItemMetadata, TaskSubscriptionEventKind};
use serde_json::Value;

fn create_project_task_resolved_status(params: &CreateProjectTaskParams) -> String {
    params
        .status
        .clone()
        .unwrap_or_else(|| "pending".to_string())
}

fn create_project_task_resolved_parent_task_key(
    params: &CreateProjectTaskParams,
) -> Option<String> {
    params
        .parent_task_key
        .as_ref()
        .map(|task_key| task_key.trim())
        .filter(|task_key| !task_key.is_empty())
        .map(|task_key| task_key.to_string())
}

fn create_project_task_metadata() -> ProjectTaskBoardItemMetadata {
    ProjectTaskBoardItemMetadata {
        kind: Some("project_task_created".to_string()),
        tool_name: Some("create_project_task".to_string()),
        updated_at: Some(chrono::Utc::now().to_rfc3339()),
        ..Default::default()
    }
}

fn update_project_task_has_meaningful_mutation(params: &UpdateProjectTaskParams) -> bool {
    params.status.is_some()
        || params.schedule.is_some()
        || params
            .title
            .as_ref()
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
        || params.description.is_some()
}

fn validate_schedule_params(
    schedule: &ProjectTaskScheduleParams,
) -> Result<(String, chrono::DateTime<chrono::Utc>, Option<i64>), AppError> {
    let kind = schedule.kind.trim().to_string();
    let next_run_at = chrono::DateTime::parse_from_rfc3339(schedule.next_run_at.trim())
        .map_err(|err| {
            AppError::BadRequest(format!(
                "Invalid schedule.next_run_at '{}': {}",
                schedule.next_run_at, err
            ))
        })?
        .with_timezone(&chrono::Utc);
    match kind.as_str() {
        "once" => {
            if schedule.interval_seconds.is_some() {
                return Err(AppError::BadRequest(
                    "Schedule kind 'once' must not set interval_seconds".to_string(),
                ));
            }
            Ok((kind, next_run_at, None))
        }
        "interval" => {
            let interval_seconds = schedule.interval_seconds.unwrap_or(0);
            if interval_seconds <= 0 {
                return Err(AppError::BadRequest(
                    "Schedule kind 'interval' requires interval_seconds > 0".to_string(),
                ));
            }
            Ok((kind, next_run_at, Some(interval_seconds)))
        }
        _ => Err(AppError::BadRequest(format!(
            "Unsupported schedule kind '{}'",
            schedule.kind
        ))),
    }
}

fn normalize_schedule_params(schedule: &ProjectTaskScheduleParams) -> ProjectTaskScheduleParams {
    let kind = schedule.kind.trim().to_string();
    let next_run_at = schedule.next_run_at.trim().to_string();
    let interval_seconds = match kind.as_str() {
        "once" => None,
        _ => schedule.interval_seconds,
    };

    ProjectTaskScheduleParams {
        kind,
        next_run_at,
        interval_seconds,
    }
}

fn update_project_task_metadata() -> ProjectTaskBoardItemMetadata {
    ProjectTaskBoardItemMetadata {
        kind: Some("project_task_updated".to_string()),
        tool_name: Some("update_project_task".to_string()),
        updated_at: Some(chrono::Utc::now().to_rfc3339()),
        ..Default::default()
    }
}

impl AgentExecutor {
    pub(crate) async fn handle_create_project_task(
        &mut self,
        params: CreateProjectTaskParams,
    ) -> Result<Value, AppError> {
        if !self.can_create_project_task_in_current_mode() {
            return Err(AppError::BadRequest(
                "create_project_task is available only to the coordinator thread or a user-facing conversation thread".to_string(),
            ));
        }

        let title = params.title.trim().to_string();
        if title.is_empty() {
            return Err(AppError::BadRequest(
                "create_project_task requires a non-empty title".to_string(),
            ));
        }

        let board_item_id = self.ctx.app_state.sf.next_id()? as i64;
        let parent_task_key = create_project_task_resolved_parent_task_key(&params);
        let description = params.description.clone();
        let status = create_project_task_resolved_status(&params);
        let metadata = create_project_task_metadata();
        let schedule = params
            .schedule
            .as_ref()
            .map(normalize_schedule_params)
            .as_ref()
            .map(validate_schedule_params)
            .transpose()?;

        let auto_subscribe = self.is_conversation_thread && params.auto_subscribe.unwrap_or(true);
        let subscribe_for_thread_id = if auto_subscribe {
            Some(self.ctx.thread_id)
        } else {
            None
        };

        let board_item = self
            .create_project_task_board_item(
                board_item_id,
                title,
                description,
                status,
                parent_task_key.clone(),
                metadata,
                schedule,
                subscribe_for_thread_id,
            )
            .await?;

        let project_workspace_path = format!("/project_workspace/tasks/{}", board_item.task_key);
        let subscribed = subscribe_for_thread_id.is_some();

        Ok(serde_json::json!({
            "success": true,
            "tool": "create_project_task",
            "created_task_key": board_item.task_key,
            "task_key": board_item.task_key,
            "parent_task_key": parent_task_key,
            "created": true,
            "routed_to_coordinator": true,
            "subscribed": subscribed,
            "created_board_item_id": board_item.id.to_string(),
            "board_item_id": board_item.id.to_string(),
            "project_workspace_path": project_workspace_path,
        }))
    }

    pub(crate) async fn handle_update_project_task(
        &mut self,
        params: UpdateProjectTaskParams,
    ) -> Result<Value, AppError> {
        if !self.can_write_project_task_board_in_current_mode() {
            return Err(AppError::BadRequest(
                "update_project_task is available only to the coordinator thread, while handling an assignment event, or from a user-facing conversation thread".to_string(),
            ));
        }

        if !update_project_task_has_meaningful_mutation(&params) {
            return Err(AppError::BadRequest(
                "update_project_task requires at least one meaningful change. If no changes are to be made, this tool is not useful, and should not be called.".to_string(),
            ));
        }

        if self.is_conversation_thread && !self.effective_is_coordinator_thread() {
            return self
                .handle_update_project_task_from_conversation(params)
                .await;
        }

        if let Some(next_status) = params.status.as_deref() {
            super::status_machine::validate_status_for_role(
                self.current_thread_role(),
                next_status,
            )?;
            super::status_machine::validate_terminal_payload_shape(next_status, &params)?;
            if next_status == "completed" {
                if let Some(artifacts) = params.artifacts.as_deref() {
                    for artifact in artifacts {
                        if !self.filesystem.exists(&artifact.path).await? {
                            return Err(AppError::BadRequest(format!(
                                "update_project_task: declared artifact `{}` is not present in \
                                 the task sandbox. Write the file before marking the task completed.",
                                artifact.path
                            )));
                        }
                    }
                }
                self.persist_completion_handoff(&params).await?;
            }
        }

        let task_key = params.task_key.clone();

        self.guard_routing_requires_task_brief(&task_key, "update_project_task")
            .await?;

        let board_item = self
            .update_project_task_board_item(
                task_key.clone(),
                params.status.clone(),
                update_project_task_metadata(),
                params
                    .schedule
                    .as_ref()
                    .map(normalize_schedule_params)
                    .as_ref()
                    .map(validate_schedule_params)
                    .transpose()?,
            )
            .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": "update_project_task",
            "task_key": task_key,
            "updated": true,
            "board_item_id": board_item.id.to_string(),
            "project_workspace_path": format!("/project_workspace/tasks/{}", task_key),
        }))
    }

    async fn handle_update_project_task_from_conversation(
        &mut self,
        params: UpdateProjectTaskParams,
    ) -> Result<Value, AppError> {
        let task_key = params.task_key.trim().to_string();
        if task_key.is_empty() {
            return Err(AppError::BadRequest(
                "update_project_task requires a task_key".to_string(),
            ));
        }

        if params.status.is_some()
            || params.schedule.is_some()
            || params.result_summary.is_some()
            || params.artifacts.is_some()
        {
            return Err(AppError::BadRequest(
                "update_project_task from a conversation thread can only revise `title` or `description` — status, schedule, result_summary, and artifacts are coordinator-only.".to_string(),
            ));
        }

        let board_id = self.ensure_project_task_board_id().await?;
        let board_item = queries::GetProjectTaskBoardItemByTaskKeyQuery::new(board_id, &task_key)
            .execute_with_db(
                self.ctx
                    .app_state
                    .db_router
                    .reader(common::ReadConsistency::Strong),
            )
            .await?
            .ok_or_else(|| {
                AppError::BadRequest(format!("Task `{task_key}` not found on this project board"))
            })?;

        let project_id = self.ctx.get_thread().await?.project_id;
        let project =
            queries::GetActorProjectByIdQuery::new(project_id, self.ctx.agent.deployment_id)
                .execute_with_db(
                    self.ctx
                        .app_state
                        .db_router
                        .reader(common::ReadConsistency::Strong),
                )
                .await?
                .ok_or_else(|| {
                    AppError::Internal(format!("Project {project_id} not found for task update"))
                })?;

        let outcome = commands::ApplyBoardItemEditCommand {
            deployment_id: self.ctx.agent.deployment_id,
            board_item_id: board_item.id,
            coordinator_thread_id: project.coordinator_thread_id,
            title: params.title.clone(),
            description: params.description.clone(),
            status: None,
            preempt_summary: "Preempted by user revision from conversation thread.",
            fanout_subscriptions: true,
        }
        .execute(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
        .await?;

        if outcome.routed || outcome.preempted || outcome.subscribers_notified > 0 {
            commands::event_log::nudge_dispatcher(&self.ctx.app_state.nats_client).await;
        }
        self.refresh_project_task_board_items().await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": "update_project_task",
            "task_key": task_key,
            "updated": !outcome.changed_fields.is_empty(),
            "board_item_id": outcome.item.id.to_string(),
            "project_workspace_path": format!("/project_workspace/tasks/{}", task_key),
            "changed_fields": outcome.changed_fields.iter().map(|c| serde_json::json!({
                "field": c.field,
                "from": c.from,
                "to": c.to,
            })).collect::<Vec<_>>(),
            "preempted_running_execution": outcome.preempted,
            "routed_to_coordinator": outcome.routed,
        }))
    }

    pub(crate) async fn handle_subscribe_to_task(
        &mut self,
        params: SubscribeToTaskParams,
    ) -> Result<Value, AppError> {
        if !self.is_conversation_thread {
            return Err(AppError::BadRequest(
                "subscribe_to_task is available only to user-facing conversation threads"
                    .to_string(),
            ));
        }

        let task_key = params.task_key.trim().to_string();
        if task_key.is_empty() {
            return Err(AppError::BadRequest(
                "subscribe_to_task requires a task_key".to_string(),
            ));
        }

        let board_id = self.ensure_project_task_board_id().await?;
        let board_item = queries::GetProjectTaskBoardItemByTaskKeyQuery::new(board_id, &task_key)
            .execute_with_db(
                self.ctx
                    .app_state
                    .db_router
                    .reader(common::ReadConsistency::Strong),
            )
            .await?
            .ok_or_else(|| {
                AppError::BadRequest(format!("Task `{task_key}` not found on this project board"))
            })?;

        let event_kinds = match params.event_kinds.as_ref() {
            Some(values) if !values.is_empty() => {
                let mut parsed = Vec::with_capacity(values.len());
                for raw in values {
                    let kind = TaskSubscriptionEventKind::from_status(raw.trim()).ok_or_else(
                        || {
                            AppError::BadRequest(format!(
                                "subscribe_to_task: unsupported event_kind `{raw}` (allowed: completed, blocked, cancelled)"
                            ))
                        },
                    )?;
                    if !parsed.contains(&kind) {
                        parsed.push(kind);
                    }
                }
                parsed
            }
            _ => TaskSubscriptionEventKind::defaults(),
        };

        let subscription = commands::UpsertAgentThreadTaskSubscriptionCommand {
            deployment_id: self.ctx.agent.deployment_id,
            thread_id: self.ctx.thread_id,
            board_item_id: board_item.id,
            event_kinds: event_kinds.clone(),
        }
        .execute(self.ctx.app_state.db_router.writer())
        .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": "subscribe_to_task",
            "task_key": task_key,
            "board_item_id": subscription.board_item_id.to_string(),
            "event_kinds": event_kinds.iter().map(|k| k.as_str()).collect::<Vec<_>>(),
            "project_workspace_path": format!("/project_workspace/tasks/{}", task_key),
        }))
    }

    pub(crate) async fn handle_unsubscribe_from_task(
        &mut self,
        params: UnsubscribeFromTaskParams,
    ) -> Result<Value, AppError> {
        if !self.is_conversation_thread {
            return Err(AppError::BadRequest(
                "unsubscribe_from_task is available only to user-facing conversation threads"
                    .to_string(),
            ));
        }

        let task_key = params.task_key.trim().to_string();
        if task_key.is_empty() {
            return Err(AppError::BadRequest(
                "unsubscribe_from_task requires a task_key".to_string(),
            ));
        }

        let board_id = self.ensure_project_task_board_id().await?;
        let board_item = queries::GetProjectTaskBoardItemByTaskKeyQuery::new(board_id, &task_key)
            .execute_with_db(
                self.ctx
                    .app_state
                    .db_router
                    .reader(common::ReadConsistency::Strong),
            )
            .await?
            .ok_or_else(|| {
                AppError::BadRequest(format!("Task `{task_key}` not found on this project board"))
            })?;

        let removed = commands::DeleteAgentThreadTaskSubscriptionCommand {
            thread_id: self.ctx.thread_id,
            board_item_id: board_item.id,
        }
        .execute(self.ctx.app_state.db_router.writer())
        .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": "unsubscribe_from_task",
            "task_key": task_key,
            "board_item_id": board_item.id.to_string(),
            "removed": removed,
        }))
    }

    pub(crate) async fn handle_get_project_task(
        &mut self,
        params: GetProjectTaskParams,
    ) -> Result<Value, AppError> {
        let task_key = params.task_key.trim().to_string();
        if task_key.is_empty() {
            return Err(AppError::BadRequest(
                "get_project_task requires a task_key".to_string(),
            ));
        }

        let board_id = self.ensure_project_task_board_id().await?;
        let reader = self
            .ctx
            .app_state
            .db_router
            .reader(common::ReadConsistency::Strong);
        let board_item = queries::GetProjectTaskBoardItemByTaskKeyQuery::new(board_id, &task_key)
            .execute_with_db(reader)
            .await?
            .ok_or_else(|| {
                AppError::BadRequest(format!("Task `{task_key}` not found on this project board"))
            })?;

        let schedule = if let Some(sid) = board_item.schedule_id {
            queries::GetProjectTaskScheduleByIdQuery::new(sid)
                .execute_with_db(
                    self.ctx
                        .app_state
                        .db_router
                        .reader(common::ReadConsistency::Strong),
                )
                .await?
        } else {
            None
        };

        let assignments = queries::ListProjectTaskBoardItemAssignmentsQuery::new(board_item.id)
            .execute_with_db(
                self.ctx
                    .app_state
                    .db_router
                    .reader(common::ReadConsistency::Strong),
            )
            .await?;
        let latest_assignment = assignments.into_iter().max_by_key(|a| a.updated_at);

        let subscription =
            queries::GetAgentThreadTaskSubscriptionQuery::new(self.ctx.thread_id, board_item.id)
                .execute_with_db(
                    self.ctx
                        .app_state
                        .db_router
                        .reader(common::ReadConsistency::Strong),
                )
                .await?;

        let schedule_json = schedule.as_ref().map(|s| {
            serde_json::json!({
                "kind": s.schedule_kind,
                "interval_seconds": s.interval_seconds,
                "next_run_at": s.next_run_at.to_rfc3339(),
                "last_fired_at": s.last_fired_at.map(|t| t.to_rfc3339()),
                "overlap_policy": s.overlap_policy,
            })
        });

        let latest_assignment_json = latest_assignment.as_ref().map(|a| {
            serde_json::json!({
                "status": a.status,
                "role": a.assignment_role,
                "thread_id": a.thread_id.to_string(),
                "result_status": a.result_status,
                "result_summary": a.result_summary,
                "updated_at": a.updated_at.to_rfc3339(),
            })
        });

        let subscribed_event_kinds = subscription
            .as_ref()
            .map(|s| s.event_kinds.iter().map(|k| k.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();

        Ok(serde_json::json!({
            "success": true,
            "tool": "get_project_task",
            "task_key": board_item.task_key,
            "title": board_item.title,
            "description": board_item.description,
            "status": board_item.status,
            "board_item_id": board_item.id.to_string(),
            "is_recurring": board_item.schedule_id.is_some(),
            "schedule": schedule_json,
            "fired_at": board_item.fired_at.map(|t| t.to_rfc3339()),
            "completed_at": board_item.completed_at.map(|t| t.to_rfc3339()),
            "created_at": board_item.created_at.to_rfc3339(),
            "updated_at": board_item.updated_at.to_rfc3339(),
            "latest_assignment": latest_assignment_json,
            "subscribed": subscription.is_some(),
            "subscribed_event_kinds": subscribed_event_kinds,
            "project_workspace_path": format!("/project_workspace/tasks/{}", board_item.task_key),
        }))
    }

    pub(crate) async fn handle_assign_project_task(
        &mut self,
        params: AssignProjectTaskParams,
    ) -> Result<Value, AppError> {
        if !self.effective_is_coordinator_thread() {
            return Err(AppError::BadRequest(
                "assign_project_task is available only to the coordinator thread".to_string(),
            ));
        }

        if params.assignments.is_empty() {
            return Err(AppError::BadRequest(
                "assign_project_task requires at least one assignment. Use sleep when no routing change is needed.".to_string(),
            ));
        }

        if self.assign_project_task_already_called_this_run(&params.task_key) {
            return Ok(serde_json::json!({
                "success": true,
                "tool": "assign_project_task",
                "task_key": params.task_key,
                "updated": false,
                "already_assigned_this_turn": true,
                "next_step": "terminate_if_done",
                "guidance": format!(
                    "Task '{}' was already assigned earlier this turn. The lane is processing it independently — re-issuing does not accelerate it. If this is all you had to do in this turn, emit a short text response with NO tool call to end the turn (one or two sentences naming the lane and slice routed is enough). If you still have other unresolved feedback or other tasks to route on this turn, continue with those — just do not re-call assign_project_task for this task_key.",
                    params.task_key
                ),
            }));
        }

        self.guard_routing_requires_task_brief(&params.task_key, "assign_project_task")
            .await?;

        let board_id = self.ensure_project_task_board_id().await?;
        let mut board_item =
            queries::GetProjectTaskBoardItemByTaskKeyQuery::new(board_id, params.task_key.clone())
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await?
                .ok_or_else(|| {
                    AppError::BadRequest(format!(
                        "Project task '{}' was not found in the current board",
                        params.task_key
                    ))
                })?;

        let reopen = commands::ReopenBoardItemIfClosedCommand {
            board_item_id: board_item.id,
        }
        .execute_with_db(self.ctx.app_state.db_router.writer())
        .await?;
        if let Some(prior_status) = reopen {
            board_item.status = "pending".to_string();
            board_item.completed_at = None;
            tracing::info!(
                board_item_id = board_item.id,
                task_key = %board_item.task_key,
                prior_status = %prior_status,
                "assign_project_task: reopened terminal/blocked board item to pending"
            );
        }

        let changed = self
            .ensure_project_task_board_assignments(&board_item, Some(params.assignments))
            .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": "assign_project_task",
            "task_key": board_item.task_key,
            "updated": changed,
            "board_item_id": board_item.id.to_string(),
        }))
    }

    fn assign_project_task_already_called_this_run(&self, task_key: &str) -> bool {
        let current_run = self.ctx.execution_run_id;
        for conv in self.conversations.iter().rev() {
            if conv.execution_run_id != Some(current_run) {
                continue;
            }
            let models::ConversationContent::ToolResult {
                tool_name,
                status,
                input,
                ..
            } = &conv.content
            else {
                continue;
            };
            if tool_name != "assign_project_task" {
                continue;
            }
            if status != "success" {
                continue;
            }
            let prior_task_key = input.get("task_key").and_then(|v| v.as_str());
            if prior_task_key == Some(task_key) {
                return true;
            }
        }
        false
    }

    async fn guard_routing_requires_task_brief(
        &self,
        target_task_key: &str,
        tool_name: &str,
    ) -> Result<(), AppError> {
        if !self.effective_is_coordinator_thread() {
            return Ok(());
        }
        let Some(active_board_item_id) = self.current_board_item_id() else {
            return Ok(());
        };
        let Some(active_board_item) =
            queries::GetProjectTaskBoardItemByIdQuery::new(active_board_item_id)
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await?
        else {
            return Ok(());
        };
        if active_board_item.task_key != target_task_key {
            return Ok(());
        }
        if self.task_brief_is_ready().await? {
            return Ok(());
        }
        Err(AppError::BadRequest(format!(
            "{} blocked: `/task/TASK.md` is missing or too thin for task {}. Write the operative task brief with `write_file` before routing this task so the executor has a concrete contract.",
            tool_name, target_task_key
        )))
    }

    async fn maybe_compact_task_journal(&mut self) -> Result<bool, AppError> {
        let existing = match self
            .filesystem
            .read_file_bytes(TASK_WORKSPACE_JOURNAL_FILE)
            .await
        {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(AppError::NotFound(_)) => return Ok(false),
            Err(e) => return Err(e),
        };
        let Some(inputs) = prepare_journal_compaction(&existing) else {
            return Ok(false);
        };

        let activity = if inputs.outcomes_by_actor.is_empty() {
            String::new()
        } else {
            match self.summarise_actor_outcomes_via_weak_llm(&inputs).await {
                Ok(text) if !text.trim().is_empty() => text,
                Ok(_) => {
                    tracing::warn!(
                        thread_id = self.ctx.thread_id,
                        "journal compaction: weak LLM returned empty body; using rule-only fallback"
                    );
                    render_rule_only_activity(&inputs)
                }
                Err(err) => {
                    tracing::warn!(
                        thread_id = self.ctx.thread_id,
                        error = %err,
                        "journal compaction: weak LLM call failed; using rule-only fallback"
                    );
                    render_rule_only_activity(&inputs)
                }
            }
        };

        let new_content = finalize_journal_compaction(&existing, &inputs, &activity);
        self.filesystem
            .write_file(TASK_WORKSPACE_JOURNAL_FILE, &new_content, false)
            .await?;
        Ok(true)
    }

    async fn summarise_actor_outcomes_via_weak_llm(
        &self,
        inputs: &CheckpointInputs,
    ) -> Result<String, AppError> {
        let system_prompt = "You compress agent activity into per-actor summaries for a task journal checkpoint. \
You receive a list of actors and the outcomes each one produced over many turns. Output a markdown bullet list, one bullet per actor:\n\
- **actor_name** (N turns): 1–3 sentence summary of what they did and the final state.\n\n\
Rules:\n\
- Group by what the actor accomplished, not by turn order.\n\
- If a later outcome supersedes an earlier one (e.g. first noted X was broken, then later fixed it) — describe the final state. Mention the journey only when it matters.\n\
- Preserve concrete state changes verbatim (\"rotated X\", \"approved Y\", \"deleted Z\").\n\
- Past tense. Tight. Roughly 150 chars per actor bullet.\n\
- Output ONLY the bullet list. No preface, no JSON, no code fences.";

        let mut user_message = String::from(
            "Outcomes by actor (turn index in the compaction window, timestamp, outcome):\n\n",
        );
        for (actor, turns) in &inputs.outcomes_by_actor {
            user_message.push_str(&format!("{}:\n", actor));
            for (idx, ts, text) in turns {
                user_message.push_str(&format!("  - turn {idx} @ {ts}: {text}\n"));
            }
            user_message.push('\n');
        }
        user_message.push_str("Output the markdown bullet list (one bullet per actor).");

        let request = SemanticLlmRequest {
            system_prompt: system_prompt.to_string(),
            messages: vec![SemanticLlmMessage::text("user", user_message)],
            response_json_schema: serde_json::Value::Null,
            temperature: None,
            max_output_tokens: Some(2048),
            reasoning_effort: None,
            forced_tool_names: None,
        };

        self.create_weak_llm()
            .await?
            .generate_text_from_prompt(request)
            .await
            .map(|output| output.text)
            .map_err_internal("journal compaction summary failed")
    }

    /// Order matters: journal first (idempotent by marker), then DB. On
    /// partial failure the agent retries the whole call; the marker
    /// prevents a duplicate journal entry.
    async fn persist_completion_handoff(
        &mut self,
        params: &UpdateProjectTaskParams,
    ) -> Result<(), AppError> {
        let Some(assignment_id) = self.current_assignment_id() else {
            return Ok(());
        };

        // Best-effort — never block the turn on memory housekeeping.
        if let Err(err) = self.maybe_compact_task_journal().await {
            tracing::warn!(
                thread_id = self.ctx.thread_id,
                assignment_id,
                error = %err,
                "journal compaction failed before handoff append; proceeding with oversized journal"
            );
        }

        let handoff = HandoffPayload {
            findings: params
                .findings
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            cautions: params
                .cautions
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            next: params
                .next
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        };

        let outcome = params
            .result_summary
            .as_deref()
            .map(str::trim)
            .unwrap_or("");
        let artifacts: Vec<dto::json::TaskArtifact> = params
            .artifacts
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|a| !a.path.trim().is_empty())
            .collect();
        let artifact_paths: Vec<String> = artifacts.iter().map(|a| a.path.clone()).collect();

        append_journal_handoff_entry(
            &self.filesystem,
            assignment_id,
            &self.ctx.agent.name,
            "completed",
            outcome,
            &handoff,
            &artifact_paths,
            chrono::Utc::now(),
        )
        .await?;

        let payload_value =
            serde_json::to_value(&handoff).map_err_internal("serialize handoff payload")?;
        commands::WriteAssignmentResultPayloadCommand::new(assignment_id, payload_value)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;

        if let Some(board_item_id) = self.current_board_item_id() {
            let entry = serde_json::json!({
                "at": chrono::Utc::now().to_rfc3339(),
                "assignment_id": assignment_id.to_string(),
                "by_thread_id": self.ctx.thread_id.to_string(),
                "by_agent_name": self.ctx.agent.name,
                "result_summary": outcome,
                "artifacts": artifacts,
                "findings": handoff.findings,
                "cautions": handoff.cautions,
                "next": handoff.next,
            });
            commands::AppendBoardItemDeliverableCommand::new(board_item_id, entry)
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await?;
        }

        Ok(())
    }
}
