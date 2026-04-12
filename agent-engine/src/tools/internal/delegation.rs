use super::ToolExecutor;
use commands::{
    CreateAgentThreadCommand, UpdateAgentThreadCommand, UpsertThreadAgentAssignmentCommand,
};
use common::error::AppError;
use dto::json::agent_executor::{CreateThreadParams, ListThreadsParams, SleepParams, UpdateThreadParams};
use models::AiTool;
use tokio::time::{sleep, Duration};
use serde_json::Value;

const MAX_CUSTOM_THREAD_INSTRUCTION_WORDS: usize = 160;
const MAX_CUSTOM_THREAD_INSTRUCTION_CHARS: usize = 1200;

fn thread_identity_is_coordinator(
    title: &str,
    thread_purpose: &str,
    responsibility: Option<&str>,
) -> bool {
    thread_purpose == models::agent_thread::purpose::COORDINATOR
        || title.eq_ignore_ascii_case("coordinator")
        || responsibility
            .map(|value| {
                value.eq_ignore_ascii_case("project coordinator")
                    || value.eq_ignore_ascii_case("coordinator")
            })
            .unwrap_or(false)
}

fn preview_text_by_words(input: Option<&str>, max_words: usize) -> Option<String> {
    let input = input?.trim();
    if input.is_empty() {
        return None;
    }

    let words = input.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        return None;
    }

    let preview = words
        .iter()
        .take(max_words)
        .copied()
        .collect::<Vec<_>>()
        .join(" ");

    if words.len() > max_words {
        Some(format!("{preview} ..."))
    } else {
        Some(preview)
    }
}

fn validate_custom_thread_instructions(input: &str) -> Result<String, AppError> {
    let normalized = input.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err(AppError::BadRequest(
            "Thread system_instructions cannot be empty".to_string(),
        ));
    }

    let word_count = normalized.split_whitespace().count();
    if word_count > MAX_CUSTOM_THREAD_INSTRUCTION_WORDS
        || normalized.chars().count() > MAX_CUSTOM_THREAD_INSTRUCTION_CHARS
    {
        return Err(AppError::BadRequest(format!(
            "Thread system_instructions must stay concise and durable (max {} words). Do not paste a task brief into this field.",
            MAX_CUSTOM_THREAD_INSTRUCTION_WORDS
        )));
    }

    let lower = normalized.to_ascii_lowercase();
    let blocked_fragments = [
        "json payload",
        "tool call",
        "execute the tool",
        "stop reading",
        "the end",
        "goodbye",
        "good luck",
        "have a nice day",
        "over and out",
        "end of transmission",
        "no more instructions",
        "proceed to execution",
        "proceed to tool call",
    ];
    if blocked_fragments
        .iter()
        .any(|fragment| lower.contains(fragment))
    {
        return Err(AppError::BadRequest(
            "Thread system_instructions must describe durable lane behavior only. Remove meta chatter, tool-call text, and conversational filler."
                .to_string(),
        ));
    }

    if lower.contains("http://") || lower.contains("https://") {
        return Err(AppError::BadRequest(
            "Thread system_instructions must stay durable across tasks. Remove task-specific URLs and keep this field policy-level."
                .to_string(),
        ));
    }

    Ok(normalized)
}

impl ToolExecutor {
    pub(super) async fn execute_sleep(
        &self,
        tool: &AiTool,
        params: SleepParams,
    ) -> Result<Value, AppError> {
        let bounded_ms = params.duration_ms.min(10_000);
        sleep(Duration::from_millis(bounded_ms)).await;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "slept_ms": bounded_ms,
            "requested_ms": params.duration_ms,
            "reason": params.reason
        }))
    }

    pub(super) async fn execute_list_threads(
        &self,
        tool: &AiTool,
        params: ListThreadsParams,
    ) -> Result<Value, AppError> {
        let current_thread = self.ctx.get_thread().await?;
        let mut query = queries::ListAgentThreadsQuery::new(
            self.agent().deployment_id,
            current_thread.project_id,
        );
        if params.include_archived {
            query = query.include_archived();
        }
        let mut threads = query
            .execute_with_db(
                self.app_state()
                    .db_router
                    .reader(common::db_router::ReadConsistency::Strong),
            )
            .await?;

        if !params.include_conversation_threads {
            threads.retain(|thread| {
                thread.thread_purpose != models::agent_thread::purpose::CONVERSATION
            });
        }

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "threads": threads.into_iter().map(|thread| serde_json::json!({
                "thread_id": thread.id.to_string(),
                "title": thread.title,
                "thread_purpose": if thread_identity_is_coordinator(
                    &thread.title,
                    &thread.thread_purpose,
                    thread.responsibility.as_deref(),
                ) {
                    models::agent_thread::purpose::COORDINATOR.to_string()
                } else {
                    thread.thread_purpose
                },
                "responsibility": thread.responsibility,
                "status": thread.status,
                "last_activity_at": thread.last_activity_at.to_rfc3339(),
                "completed_at": thread.completed_at.map(|value| value.to_rfc3339()),
                "accepts_assignments": thread.accepts_assignments,
                "reusable": thread.reusable,
                "capability_tags": thread.capability_tags,
                "system_instructions_preview": preview_text_by_words(thread.system_instructions.as_deref(), 100),
            })).collect::<Vec<_>>(),
        }))
    }

    pub(super) async fn execute_create_thread(
        &self,
        tool: &AiTool,
        params: CreateThreadParams,
    ) -> Result<Value, AppError> {
        let current_thread = self.ctx.get_thread().await?;
        if !thread_identity_is_coordinator(
            &current_thread.title,
            &current_thread.thread_purpose,
            current_thread.responsibility.as_deref(),
        ) {
            return Err(AppError::BadRequest(
                "create_thread is only available to coordinator threads".to_string(),
            ));
        }

        let title = params.title.trim().to_string();
        if title.is_empty() {
            return Err(AppError::BadRequest(
                "create_thread requires a title".to_string(),
            ));
        }

        let thread_purpose = models::agent_thread::purpose::EXECUTION.to_string();
        let responsibility = params
            .responsibility
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let requested_agent_name = params
            .assigned_agent_name
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let capability_tags = params.capability_tags.unwrap_or_default();
        let reusable = params.reusable.unwrap_or(true);
        let accepts_assignments = params.accepts_assignments.unwrap_or(true);
        let system_instructions = match params
            .system_instructions
            .filter(|value| !value.trim().is_empty())
        {
            Some(value) => validate_custom_thread_instructions(&value)?,
            None => default_thread_instructions(&title, &thread_purpose, responsibility.as_deref()),
        };
        let available_sub_agents = if let Some(sub_agent_ids) = &self.agent().sub_agents {
            if sub_agent_ids.is_empty() {
                Vec::new()
            } else {
                queries::GetAiAgentsByIdsQuery::new(
                    self.agent().deployment_id,
                    sub_agent_ids.clone(),
                )
                .execute_with_db(
                    self.app_state()
                        .db_router
                        .reader(common::db_router::ReadConsistency::Strong),
                )
                .await?
            }
        } else {
            Vec::new()
        };
        let (assigned_agent_id, assigned_agent_name) = match requested_agent_name {
            Some(requested_agent_name) => {
                if requested_agent_name.eq_ignore_ascii_case(&self.agent().name) {
                    (self.agent().id, self.agent().name.clone())
                } else if let Some(agent) = available_sub_agents
                    .iter()
                    .find(|agent| agent.name.eq_ignore_ascii_case(&requested_agent_name))
                {
                    (agent.id, agent.name.clone())
                } else {
                    let mut available_agent_names = vec![self.agent().name.clone()];
                    available_agent_names.extend(
                        available_sub_agents
                            .iter()
                            .map(|agent| agent.name.clone())
                            .collect::<Vec<_>>(),
                    );
                    return Err(AppError::BadRequest(format!(
                        "assigned_agent_name must be the current agent or one of its sub-agents. Available agents: {}",
                        available_agent_names.join(", ")
                    )));
                }
            }
            None => (self.agent().id, self.agent().name.clone()),
        };

        let thread_id = self.app_state().sf.next_id()? as i64;
        let mut command = CreateAgentThreadCommand::new(
            thread_id,
            self.agent().deployment_id,
            current_thread.actor_id,
            current_thread.project_id,
            title.clone(),
            thread_purpose.clone(),
            models::AgentThreadStatus::Idle.to_string(),
        )
        .with_thread_purpose(thread_purpose)
        .with_system_instructions(system_instructions)
        .with_capability_tags(capability_tags.clone());

        if let Some(responsibility) = responsibility.clone() {
            command = command.with_responsibility(responsibility);
        }
        if reusable {
            command = command.mark_reusable();
        }
        if accepts_assignments {
            command = command.allow_assignments();
        }
        if let Some(metadata) = params.metadata {
            command = command.with_metadata(metadata);
        }

        let created = command
            .execute_with_db(self.app_state().db_router.writer())
            .await?;
        UpsertThreadAgentAssignmentCommand::new(created.id, assigned_agent_id)
            .execute_with_db(self.app_state().db_router.writer())
            .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "created_thread_id": created.id.to_string(),
            "created_thread_title": created.title,
            "created_thread_purpose": created.thread_purpose,
            "thread": {
                "thread_id": created.id.to_string(),
                "title": created.title,
                "thread_purpose": created.thread_purpose,
                "responsibility": created.responsibility,
                "status": created.status,
                "accepts_assignments": created.accepts_assignments,
                "reusable": created.reusable,
                "capability_tags": created.capability_tags,
                "assigned_agent_id": assigned_agent_id.to_string(),
                "assigned_agent_name": assigned_agent_name,
            }
        }))
    }

    pub(super) async fn execute_update_thread(
        &self,
        tool: &AiTool,
        params: UpdateThreadParams,
    ) -> Result<Value, AppError> {
        let current_thread = self.ctx.get_thread().await?;
        if !thread_identity_is_coordinator(
            &current_thread.title,
            &current_thread.thread_purpose,
            current_thread.responsibility.as_deref(),
        ) {
            return Err(AppError::BadRequest(
                "update_thread is only available to coordinator threads".to_string(),
            ));
        }

        let target_thread_id = params.thread_id.into_inner();
        let existing =
            queries::GetAgentThreadByIdQuery::new(target_thread_id, self.agent().deployment_id)
                .execute_with_db(
                    self.app_state()
                        .db_router
                        .reader(common::db_router::ReadConsistency::Strong),
                )
                .await?
                .ok_or_else(|| AppError::NotFound("Thread not found".to_string()))?;

        if existing.project_id != current_thread.project_id {
            return Err(AppError::BadRequest(
                "update_thread can only modify threads in the current project".to_string(),
            ));
        }

        let mut command =
            UpdateAgentThreadCommand::new(target_thread_id, self.agent().deployment_id);
        if let Some(title) = params
            .title
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            command = command.with_title(title);
        }
        if params.responsibility.is_some() {
            command = command.with_responsibility(
                params
                    .responsibility
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
            );
        }
        if params.system_instructions.is_some() {
            command = command.with_system_instructions(
                params
                    .system_instructions
                    .map(|value| validate_custom_thread_instructions(&value))
                    .transpose()?
                    .filter(|value| !value.is_empty()),
            );
        }
        if let Some(reusable) = params.reusable {
            command = command.with_reusable(reusable);
        }
        if let Some(accepts_assignments) = params.accepts_assignments {
            command = command.with_accepts_assignments(accepts_assignments);
        }
        if let Some(capability_tags) = params.capability_tags {
            command = command.with_capability_tags(capability_tags);
        }
        if let Some(metadata) = params.metadata {
            command = command.with_metadata(metadata);
        }

        let updated = command
            .execute_with_db(self.app_state().db_router.writer())
            .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "thread": {
                "thread_id": updated.id.to_string(),
                "title": updated.title,
                "thread_purpose": updated.thread_purpose,
                "responsibility": updated.responsibility,
                "status": updated.status,
                "accepts_assignments": updated.accepts_assignments,
                "reusable": updated.reusable,
                "capability_tags": updated.capability_tags,
            }
        }))
    }
}

fn default_thread_instructions(
    title: &str,
    thread_purpose: &str,
    responsibility: Option<&str>,
) -> String {
    let mut lines = vec![format!(
        "You are the '{}' thread. Operate as a stable reusable {} lane.",
        title, thread_purpose
    )];
    if let Some(responsibility) = responsibility {
        lines.push(format!("Primary responsibility: {}.", responsibility));
    }
    lines.push(
        "Follow assignment instructions and the project task board as the workflow source of truth."
            .to_string(),
    );
    lines.push(
        "When a mounted task workspace is present, keep notes in `/task/notes/`, artifacts in `/task/artifacts/`, and handoffs in `/task/handoffs/`."
            .to_string(),
    );
    lines.push(
        "Verify important claims before concluding, keep outputs structured and decision-useful, and state uncertainty explicitly instead of guessing."
            .to_string(),
    );
    lines.join("\n")
}
