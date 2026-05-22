use super::ToolExecutor;
use crate::sandbox::ExecRequest;
use commands::{
    CreateAgentThreadCommand, CreateProjectTaskBoardItemAssignmentCommand,
    CreateProjectTaskBoardItemCommand, UpdateAgentThreadCommand,
    UpsertAgentThreadTaskSubscriptionCommand, UpsertThreadAgentAssignmentCommand,
};
use common::error::AppError;
use dto::json::agent_executor::{
    CreateThreadParams, DelegateTaskInputMount, DelegateTaskParams, ListThreadsParams, SleepParams,
    UpdateThreadParams,
};
use models::AiTool;
use serde_json::Value;
use std::collections::BTreeMap;
use tokio::time::{Duration, sleep};

const MAX_CUSTOM_THREAD_INSTRUCTION_WORDS: usize = 160;
const MAX_CUSTOM_THREAD_INSTRUCTION_CHARS: usize = 1200;
const MIN_LANE_RESPONSIBILITY_CHARS: usize = 30;
const MIN_LANE_RESPONSIBILITY_WORDS: usize = 4;

/// Reject single-word / two-word generic labels and length-padded gaming.
/// Combined floor: {chars} catches "research" / "marketing research", and
/// {words} catches "doing-research-work-here" (one compound token padded out).
fn validate_lane_responsibility(input: Option<&str>) -> Result<String, AppError> {
    let normalized = input
        .map(|v| v.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    if normalized.is_empty() {
        return Err(AppError::BadRequest(
            "create_thread / update_thread requires `responsibility` — a durable routing label naming what this lane owns. At least 30 characters and 4 words. Examples: 'competitor pricing research for SaaS landing pages', 'final approval before publishing customer-facing collateral'.".to_string(),
        ));
    }
    let char_count = normalized.chars().count();
    let word_count = normalized.split_whitespace().count();
    if char_count < MIN_LANE_RESPONSIBILITY_CHARS || word_count < MIN_LANE_RESPONSIBILITY_WORDS {
        return Err(AppError::BadRequest(format!(
            "responsibility ({normalized:?}, {char_count} chars / {word_count} words) is too generic. Minimum: {MIN_LANE_RESPONSIBILITY_CHARS} chars AND {MIN_LANE_RESPONSIBILITY_WORDS} words. It must differentiate this lane from siblings — name the *scope* it owns, not just the domain. Bad: 'research', 'marketing research', 'review'. Good: 'competitor pricing research for SaaS landing pages', 'pricing-page copy review with conversion focus'."
        )));
    }
    Ok(normalized)
}

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

/// Stopwords stripped before computing lane similarity. They carry no semantic
/// distinctness ("Research Lane" vs "Marketing Lane" should not match because
/// they share "lane") and would otherwise inflate Jaccard scores.
const LANE_SIMILARITY_STOPWORDS: &[&str] = &[
    "a",
    "an",
    "and",
    "for",
    "of",
    "or",
    "the",
    "to",
    "with",
    "lane",
    "thread",
    "agent",
    "service",
    "team",
    "specialist",
    "executor",
    "execution",
    "worker",
    "helper",
    "support",
    "general",
    "subagent",
];

fn tokenize_for_similarity(input: &str) -> std::collections::HashSet<String> {
    input
        .to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .filter(|t| !LANE_SIMILARITY_STOPWORDS.contains(t))
        .map(|t| t.to_string())
        .collect()
}

fn jaccard_similarity(
    a: &std::collections::HashSet<String>,
    b: &std::collections::HashSet<String>,
) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

const LANE_SIMILARITY_REJECT_THRESHOLD: f64 = 0.80;
const MIN_DELEGATE_DESCRIPTION_CHARS: usize = 80;
const MIN_DELEGATE_DESCRIPTION_WORDS: usize = 14;

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

fn validate_delegate_description(input: Option<&str>) -> Result<String, AppError> {
    let normalized = input
        .map(|v| v.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    let char_count = normalized.chars().count();
    let word_count = normalized.split_whitespace().count();
    if char_count < MIN_DELEGATE_DESCRIPTION_CHARS || word_count < MIN_DELEGATE_DESCRIPTION_WORDS {
        return Err(AppError::BadRequest(format!(
            "delegate_task requires a clear brief with boundaries and an expected output (minimum {MIN_DELEGATE_DESCRIPTION_CHARS} chars and {MIN_DELEGATE_DESCRIPTION_WORDS} words). Include what to inspect, what to ignore, and what file to write under `/delegated_workspace/`."
        )));
    }
    let lower = normalized.to_ascii_lowercase();
    let has_output_boundary = lower.contains("/delegated_workspace")
        || lower.contains("output")
        || lower.contains("deliverable")
        || lower.contains("write")
        || lower.contains("produce");
    let has_scope_boundary = lower.contains("inspect")
        || lower.contains("analyze")
        || lower.contains("review")
        || lower.contains("read")
        || lower.contains("scope");
    if !has_output_boundary || !has_scope_boundary {
        return Err(AppError::BadRequest(
            "delegate_task brief must clearly state both the input/scope to inspect and the expected output/deliverable. Name the `/delegated_workspace/` output path when possible."
                .to_string(),
        ));
    }
    Ok(normalized)
}

fn validate_delegate_input_mounts(
    mounts: Option<Vec<DelegateTaskInputMount>>,
    conv_thread_id: i64,
) -> Result<Vec<serde_json::Value>, AppError> {
    let mut resolved = Vec::new();
    let mut seen_aliases = std::collections::HashSet::new();
    let mut seen_paths = std::collections::HashSet::new();
    for mount in mounts.unwrap_or_default() {
        let path = mount.path.trim().trim_end_matches('/').to_string();
        if path == "/workspace" || path == "workspace" {
            return Err(AppError::BadRequest(
                "delegate_task input_mounts.path must be an explicit subfolder under `/workspace/`, not the workspace root."
                    .to_string(),
            ));
        }
        let Some(relative) = path
            .strip_prefix("/workspace/")
            .or_else(|| path.strip_prefix("workspace/"))
        else {
            return Err(AppError::BadRequest(format!(
                "delegate_task input_mounts.path `{path}` must be under `/workspace/`."
            )));
        };
        if relative.is_empty()
            || relative
                .split('/')
                .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        {
            return Err(AppError::BadRequest(format!(
                "delegate_task input_mounts.path `{path}` must be a normalized workspace subfolder path."
            )));
        }

        let alias = mount.alias.trim().trim_matches('/').to_string();
        if alias.is_empty()
            || alias.contains('/')
            || alias == "."
            || alias == ".."
            || !alias
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        {
            return Err(AppError::BadRequest(
                "delegate_task input_mounts.alias must be a single path segment using letters, numbers, dash, or underscore."
                    .to_string(),
            ));
        }
        if !seen_aliases.insert(alias.clone()) {
            return Err(AppError::BadRequest(format!(
                "delegate_task input_mounts alias `{alias}` is duplicated."
            )));
        }
        if !seen_paths.insert(relative.to_string()) {
            return Err(AppError::BadRequest(format!(
                "delegate_task input_mounts path `/workspace/{relative}` is duplicated."
            )));
        }

        resolved.push(serde_json::json!({
            "mount_path": format!("/delegated_inputs/{alias}"),
            "s3_relative_key": format!("persistent/{conv_thread_id}/workspace/{relative}"),
            "mode": "ro",
            "source_path": format!("/workspace/{relative}"),
            "alias": alias,
        }));
    }
    Ok(resolved)
}

impl ToolExecutor {
    async fn verify_delegate_input_mount_sources(
        &self,
        input_mounts: &[serde_json::Value],
    ) -> Result<(), AppError> {
        let sandbox = self.sandbox_handle()?;
        for mount in input_mounts {
            let Some(source_path) = mount.get("source_path").and_then(|v| v.as_str()) else {
                continue;
            };
            let result = sandbox
                .exec(ExecRequest {
                    command: vec![
                        "bash".into(),
                        "-c".into(),
                        "test -d \"$1\"".into(),
                        "_".into(),
                        source_path.to_string(),
                    ],
                    cwd: None,
                    env: BTreeMap::new(),
                    timeout: Some(Duration::from_secs(5)),
                    exec_id: None,
                })
                .await
                .map_err(|err| {
                    AppError::Internal(format!(
                        "delegate_task failed to inspect input mount source `{source_path}`: {err}"
                    ))
                })?;
            if result.exit_code != 0 {
                return Err(AppError::BadRequest(format!(
                    "delegate_task input_mounts.path `{source_path}` must exist and be a directory in the conversation workspace before delegation."
                )));
            }
        }
        Ok(())
    }

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

        let assignment_reader = self
            .app_state()
            .db_router
            .reader(common::db_router::ReadConsistency::Strong);
        let thread_ids: Vec<i64> = threads.iter().map(|thread| thread.id).collect();
        let assignments =
            queries::ListThreadAgentAssignmentsForThreadsQuery::new(thread_ids.clone())
                .execute_with_db(assignment_reader)
                .await?;
        let assignment_by_thread: std::collections::HashMap<i64, (i64, String)> = assignments
            .into_iter()
            .map(|assignment| {
                (
                    assignment.thread_id,
                    (assignment.agent_id, assignment.agent_name),
                )
            })
            .collect();

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "threads": threads.into_iter().map(|thread| {
                let assigned = assignment_by_thread.get(&thread.id);
                let (assigned_agent_id, assigned_agent_name) = match assigned {
                    Some((id, name)) => (Some(id.to_string()), Some(name.clone())),
                    None => (None, None),
                };
                serde_json::json!({
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
                    "assigned_agent_id": assigned_agent_id,
                    "assigned_agent_name": assigned_agent_name,
                    "status": thread.status,
                    "last_activity_at": thread.last_activity_at.to_rfc3339(),
                    "completed_at": thread.completed_at.map(|value| value.to_rfc3339()),
                    "accepts_assignments": thread.accepts_assignments,
                    "reusable": thread.reusable,
                    "capability_tags": thread.capability_tags,
                    "system_instructions_preview": preview_text_by_words(thread.system_instructions.as_deref(), 100),
                })
            }).collect::<Vec<_>>(),
        }))
    }

    async fn require_caller_is_project_coordinator_or_sub_agent(
        &self,
        current_thread: &models::AgentThreadState,
    ) -> Result<(), AppError> {
        let deployment_id = self.agent().deployment_id;
        let project =
            queries::GetActorProjectByIdQuery::new(current_thread.project_id, deployment_id)
                .execute_with_db(
                    self.app_state()
                        .db_router
                        .reader(common::db_router::ReadConsistency::Strong),
                )
                .await?
                .ok_or_else(|| {
                    AppError::Internal(format!(
                        "Project {} not found for thread {}",
                        current_thread.project_id, current_thread.id
                    ))
                })?;

        let Some(coordinator_thread_id) = project.coordinator_thread_id else {
            return Err(AppError::BadRequest(
                "create_thread: project has no coordinator thread configured".to_string(),
            ));
        };

        let coordinator_agent_id =
            queries::ResolveThreadExecutionAgentQuery::new(coordinator_thread_id, deployment_id)
                .execute_with_db(
                    self.app_state()
                        .db_router
                        .reader(common::db_router::ReadConsistency::Strong),
                )
                .await?
                .ok_or_else(|| {
                    AppError::Internal(format!(
                        "Coordinator thread {coordinator_thread_id} has no assigned agent"
                    ))
                })?;

        let caller_id = self.agent().id;
        if caller_id == coordinator_agent_id {
            return Ok(());
        }

        let coordinator_agents =
            queries::GetAiAgentsByIdsQuery::new(deployment_id, vec![coordinator_agent_id])
                .execute_with_db(
                    self.app_state()
                        .db_router
                        .reader(common::db_router::ReadConsistency::Strong),
                )
                .await?;

        let is_sub_agent = coordinator_agents
            .first()
            .and_then(|agent| agent.sub_agents.as_ref())
            .map(|subs| subs.contains(&caller_id))
            .unwrap_or(false);

        if !is_sub_agent {
            return Err(AppError::BadRequest(
                "create_thread: conversation threads may only spawn lanes when the calling agent is the project's coordinator or one of its sub-agents"
                    .to_string(),
            ));
        }

        Ok(())
    }

    pub(super) async fn execute_create_thread(
        &self,
        tool: &AiTool,
        params: CreateThreadParams,
    ) -> Result<Value, AppError> {
        let current_thread = self.ctx.get_thread().await?;
        let is_coordinator = thread_identity_is_coordinator(
            &current_thread.title,
            &current_thread.thread_purpose,
            current_thread.responsibility.as_deref(),
        );
        let is_conversation =
            current_thread.thread_purpose == models::agent_thread::purpose::CONVERSATION;

        if !is_coordinator && !is_conversation {
            return Err(AppError::BadRequest(
                "create_thread is only available to coordinator or conversation threads"
                    .to_string(),
            ));
        }

        if is_conversation && !is_coordinator {
            self.require_caller_is_project_coordinator_or_sub_agent(&current_thread)
                .await?;
        }

        let title = params.title.trim().to_string();
        if title.is_empty() {
            return Err(AppError::BadRequest(
                "create_thread requires a title".to_string(),
            ));
        }

        let thread_purpose = models::agent_thread::purpose::EXECUTION.to_string();
        let responsibility = Some(validate_lane_responsibility(
            params.responsibility.as_deref(),
        )?);
        let requested_agent_name = params.assigned_agent_name.trim().to_string();
        if requested_agent_name.is_empty() {
            return Err(AppError::BadRequest(
                "create_thread: `assigned_agent_name` is required and must not be empty — every lane must explicitly name its owner agent.".to_string(),
            ));
        }
        let capability_tags = params.capability_tags.unwrap_or_default();
        let reusable = params.reusable.unwrap_or(true);
        let accepts_assignments = params.accepts_assignments.unwrap_or(true);
        let system_instructions = match params
            .system_instructions
            .filter(|value| !value.trim().is_empty())
        {
            Some(value) => validate_custom_thread_instructions(&value)?,
            None => {
                let project = queries::GetActorProjectByIdQuery::new(
                    current_thread.project_id,
                    self.agent().deployment_id,
                )
                .execute_with_db(
                    self.app_state()
                        .db_router
                        .reader(common::db_router::ReadConsistency::Strong),
                )
                .await?
                .ok_or_else(|| {
                    AppError::Internal(format!(
                        "Project {} not found for thread {}",
                        current_thread.project_id, current_thread.id
                    ))
                })?;
                default_thread_instructions(&project.name, project.description.as_deref())?
            }
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
        let (assigned_agent_id, assigned_agent_name) = if requested_agent_name
            .eq_ignore_ascii_case(&self.agent().name)
        {
            (self.agent().id, self.agent().name.clone())
        } else if let Some(agent) = available_sub_agents
            .iter()
            .find(|agent| agent.name.eq_ignore_ascii_case(&requested_agent_name))
        {
            (agent.id, agent.name.clone())
        } else {
            let mut available_agent_names = vec![self.agent().name.clone()];
            available_agent_names
                .extend(available_sub_agents.iter().map(|agent| agent.name.clone()));
            return Err(AppError::BadRequest(format!(
                "assigned_agent_name must be the current agent or one of its sub-agents. Available agents: {}",
                available_agent_names.join(", ")
            )));
        };

        let proposed_signature = format!("{} {}", title, responsibility.as_deref().unwrap_or(""));
        let proposed_tokens = tokenize_for_similarity(&proposed_signature);
        if !proposed_tokens.is_empty() {
            let existing_threads = queries::ListAgentThreadsQuery::new(
                self.agent().deployment_id,
                current_thread.project_id,
            )
            .execute_with_db(
                self.app_state()
                    .db_router
                    .reader(common::db_router::ReadConsistency::Strong),
            )
            .await?;

            let mut best_match: Option<(f64, &models::AgentThread)> = None;
            for existing in existing_threads.iter() {
                if existing.id == current_thread.id {
                    continue;
                }
                if existing.thread_purpose == models::agent_thread::purpose::CONVERSATION
                    || existing.thread_purpose == models::agent_thread::purpose::COORDINATOR
                {
                    continue;
                }
                let existing_signature = format!(
                    "{} {}",
                    existing.title,
                    existing.responsibility.as_deref().unwrap_or("")
                );
                let existing_tokens = tokenize_for_similarity(&existing_signature);
                if existing_tokens.is_empty() {
                    continue;
                }
                let score = jaccard_similarity(&proposed_tokens, &existing_tokens);
                if score >= LANE_SIMILARITY_REJECT_THRESHOLD
                    && best_match.map(|(prior, _)| score > prior).unwrap_or(true)
                {
                    best_match = Some((score, existing));
                }
            }

            if let Some((score, dupe)) = best_match {
                return Err(AppError::BadRequest(format!(
                    "create_thread: proposed lane (`{}` / `{}`) is {:.0}% similar to existing lane #{} (`{}` / `{}`). Reuse it via `assign_project_task` if the responsibility matches, or differentiate this lane by giving it a more specific title and responsibility (must be <{:.0}% overlap).",
                    title,
                    responsibility.as_deref().unwrap_or(""),
                    score * 100.0,
                    dupe.id,
                    dupe.title,
                    dupe.responsibility.as_deref().unwrap_or(""),
                    LANE_SIMILARITY_REJECT_THRESHOLD * 100.0,
                )));
            }
        }

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
            let validated = validate_lane_responsibility(params.responsibility.as_deref())?;
            command = command.with_responsibility(Some(validated));
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

    pub(super) async fn execute_delegate_task(
        &self,
        tool: &AiTool,
        params: DelegateTaskParams,
    ) -> Result<Value, AppError> {
        let current_thread = self.ctx.get_thread().await?;
        if current_thread.thread_purpose != models::agent_thread::purpose::CONVERSATION {
            return Err(AppError::BadRequest(
                "delegate_task is only available to conversation threads".to_string(),
            ));
        }
        self.require_caller_is_project_coordinator_or_sub_agent(&current_thread)
            .await?;

        let title = params.title.trim().to_string();
        if title.is_empty() {
            return Err(AppError::BadRequest(
                "delegate_task requires a non-empty title".to_string(),
            ));
        }
        let description = validate_delegate_description(params.description.as_deref())?;

        let target_lane_thread_id: i64 = params.target_lane_thread_id.into();
        let lane = queries::GetAgentThreadStateQuery::new(
            target_lane_thread_id,
            self.agent().deployment_id,
        )
        .execute_with_db(
            self.app_state()
                .db_router
                .reader(common::db_router::ReadConsistency::Strong),
        )
        .await?;
        if lane.project_id != current_thread.project_id {
            return Err(AppError::BadRequest(
                "delegate_task: target lane is not in the current project".to_string(),
            ));
        }
        if lane.thread_purpose != models::agent_thread::purpose::EXECUTION {
            return Err(AppError::BadRequest(format!(
                "delegate_task: target thread {target_lane_thread_id} is not an execution lane (purpose={})",
                lane.thread_purpose
            )));
        }

        let lane_agent_id = queries::ResolveThreadExecutionAgentQuery::new(
            target_lane_thread_id,
            self.agent().deployment_id,
        )
        .execute_with_db(
            self.app_state()
                .db_router
                .reader(common::db_router::ReadConsistency::Strong),
        )
        .await?
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "delegate_task: lane thread {target_lane_thread_id} has no assigned agent"
            ))
        })?;

        let conv_thread_id = current_thread.id;
        let board_item_id = self.app_state().sf.next_id()? as i64;
        let task_key = format!("DELEGATE-{board_item_id}");
        let mount_s3_key = format!("persistent/{conv_thread_id}/workspace/delegate/{task_key}");
        let mut mounts = vec![serde_json::json!({
            "mount_path": "/delegated_workspace",
            "s3_relative_key": mount_s3_key,
            "mode": "rw",
        })];
        let input_mounts =
            validate_delegate_input_mounts(params.input_mounts.clone(), conv_thread_id)?;
        self.verify_delegate_input_mount_sources(&input_mounts)
            .await?;
        mounts.extend(input_mounts.clone());
        let mounts = serde_json::Value::Array(mounts);

        let board_id =
            crate::executor::project::lookup_or_create_project_task_board_id(&self.ctx).await?;

        let mut tx = self.app_state().db_router.writer().begin().await?;

        let board_item = CreateProjectTaskBoardItemCommand {
            id: board_item_id,
            board_id,
            task_key: task_key.clone(),
            title: title.clone(),
            description: Some(description.clone()),
            status: "pending".to_string(),
            assigned_thread_id: Some(target_lane_thread_id),
            metadata: serde_json::json!({
                "kind": "delegated_task",
                "delegated_by_thread_id": conv_thread_id.to_string(),
                "delegated_by_agent_id": self.agent().id.to_string(),
                "capability_tags": params.capability_tags.clone().unwrap_or_default(),
                "input_mounts": input_mounts.clone(),
            }),
            mounts: mounts.clone(),
            exclusive_owner_agent_id: Some(lane_agent_id),
        }
        .execute_with_db(&mut *tx)
        .await?;

        UpsertAgentThreadTaskSubscriptionCommand {
            deployment_id: self.agent().deployment_id,
            thread_id: conv_thread_id,
            board_item_id: board_item.id,
            event_kinds: models::TaskSubscriptionEventKind::defaults(),
        }
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        let assignment_id = self.app_state().sf.next_id()? as i64;
        let deps = common::deps::from_app(self.app_state()).db().nats().id();
        CreateProjectTaskBoardItemAssignmentCommand {
            id: assignment_id,
            board_item_id: board_item.id,
            thread_id: target_lane_thread_id,
            assignment_role: models::project_task_board::assignment_role::EXECUTOR.to_string(),
            status: models::project_task_board::assignment_status::AVAILABLE.to_string(),
            instructions: Some(description),
            metadata: serde_json::json!({
                "kind": "delegated_task_assignment",
                "delegated_by_thread_id": conv_thread_id.to_string(),
            }),
        }
        .execute_with_deps(&deps)
        .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "task_key": board_item.task_key,
            "board_item_id": board_item.id.to_string(),
            "target_lane_thread_id": target_lane_thread_id.to_string(),
            "assigned_agent_id": lane_agent_id.to_string(),
            "shared_workspace_path_in_conversation": format!("/workspace/delegate/{}", board_item.task_key),
            "shared_workspace_path_in_lane": "/delegated_workspace",
            "input_mounts_in_lane": input_mounts,
            "subscribed": true,
        }))
    }
}

fn default_thread_instructions(
    project_name: &str,
    project_brief: Option<&str>,
) -> Result<String, AppError> {
    templatekit::render_project_instructions(project_name, project_brief, None)
}
