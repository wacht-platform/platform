use commands::{
    CreateActorCommand, CreateActorProjectCommand, CreateAgentThreadCommand,
    CreateProjectTaskBoardItemCommand, CreateProjectTaskBoardItemEventCommand,
    DispatchThreadEventCommand, EnqueueThreadEventCommand, UpdateProjectTaskBoardItemCommand,
    UpsertThreadAgentAssignmentCommand,
};
use common::ReadConsistency;
use common::error::AppError;
use dto::json::deployment::{
    CreateActorProjectRequest, CreateActorRequest, CreateAgentThreadRequest, ExecuteAgentRequest,
    ExecuteAgentResponse, UpdateActorProjectRequest, UpdateAgentThreadRequest,
    CreateProjectTaskBoardItemRequest, UpdateProjectTaskBoardItemRequest,
};
use models::{
    Actor, ActorProject, AgentThread, AgentThreadState, ConversationRecord, ProjectTaskBoard,
    ProjectTaskBoardItem, ProjectTaskBoardItemAssignment, ProjectTaskBoardItemEvent,
    ProjectTaskBoardItemRelation, ThreadEvent, ThreadTaskEdge, ThreadTaskGraph,
    ThreadTaskGraphSummary, ThreadTaskNode,
};
use queries::{
    GetActorByIdQuery, GetActorProjectByIdQuery, GetAgentThreadByIdQuery, GetAgentThreadStateQuery,
    GetLatestThreadTaskGraphQuery, GetMcpServerByIdQuery, GetMcpServersQuery,
    GetProjectTaskBoardByIdQuery,
    GetProjectTaskBoardByProjectIdQuery, GetProjectTaskBoardItemByIdQuery, GetThreadEventByIdQuery,
    GetThreadTaskGraphByIdQuery, GetThreadTaskGraphSummaryQuery, ListActorProjectsQuery,
    ListActorsQuery, ListAgentThreadsQuery, ListAssignmentsForThreadQuery,
    ListPendingThreadEventsQuery, ListProjectTaskBoardItemAssignmentsQuery,
    ListProjectTaskBoardItemEventsQuery, ListProjectTaskBoardItemRelationsQuery,
    ListProjectTaskBoardItemsQuery, ListThreadTaskEdgesQuery, ListThreadTaskNodesQuery,
};

use crate::application::{AppState, agent_thread_execution as agent_thread_execution_app};
use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;

pub async fn list_actors(
    app_state: &AppState,
    deployment_id: i64,
    include_archived: bool,
) -> Result<Vec<Actor>, AppError> {
    let mut query = ListActorsQuery::new(deployment_id);
    if include_archived {
        query = query.include_archived();
    }

    query
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn create_actor(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateActorRequest,
) -> Result<Actor, AppError> {
    let mut command = CreateActorCommand::new(
        app_state.sf.next_id()? as i64,
        deployment_id,
        request.subject_type,
        request.external_key,
    );

    if let Some(display_name) = request.display_name {
        command = command.with_display_name(display_name);
    }
    if let Some(metadata) = request.metadata {
        command = command.with_metadata(metadata);
    }

    command.execute_with_db(app_state.db_router.writer()).await
}

pub async fn get_actor_by_id(
    app_state: &AppState,
    deployment_id: i64,
    actor_id: i64,
) -> Result<Actor, AppError> {
    GetActorByIdQuery::new(actor_id, deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?
        .ok_or_else(|| AppError::NotFound("Actor not found".to_string()))
}

pub async fn list_actor_projects(
    app_state: &AppState,
    deployment_id: i64,
    actor_id: i64,
    include_archived: bool,
) -> Result<Vec<ActorProject>, AppError> {
    let mut query = ListActorProjectsQuery::new(deployment_id, actor_id);
    if include_archived {
        query = query.include_archived();
    }

    query
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub struct CursorPage<T> {
    pub data: Vec<T>,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct TaskWorkspaceFileEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone, Serialize)]
pub struct TaskWorkspaceListing {
    pub exists: bool,
    pub files: Vec<TaskWorkspaceFileEntry>,
}

#[derive(Clone, Serialize)]
pub struct TaskWorkspaceFileContent {
    pub path: String,
    pub name: String,
    pub mime_type: String,
    pub is_text: bool,
    pub size_bytes: u64,
    pub truncated: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub content: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub content_base64: String,
}

#[derive(Clone, Serialize)]
pub struct ActorMcpServerSummary {
    pub id: i64,
    pub name: String,
    pub endpoint: String,
    pub auth_type: String,
    pub requires_user_connection: bool,
    pub connection_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connected_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone, Serialize)]
pub struct ActorMcpServerConnectResponse {
    pub auth_url: String,
}

fn encode_time_id_cursor(ts: chrono::DateTime<chrono::Utc>, id: i64) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!("{}|{}", ts.timestamp_nanos_opt().unwrap_or(0), id))
}

fn decode_time_id_cursor(cursor: &str) -> Result<Option<(chrono::DateTime<chrono::Utc>, i64)>, AppError> {
    use base64::Engine;
    if cursor.trim().is_empty() {
        return Ok(None);
    }
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| AppError::BadRequest("invalid cursor".to_string()))?;
    let raw = String::from_utf8(decoded)
        .map_err(|_| AppError::BadRequest("invalid cursor".to_string()))?;
    let mut parts = raw.splitn(2, '|');
    let nanos = parts
        .next()
        .ok_or_else(|| AppError::BadRequest("invalid cursor".to_string()))?
        .parse::<i64>()
        .map_err(|_| AppError::BadRequest("invalid cursor".to_string()))?;
    let id = parts
        .next()
        .ok_or_else(|| AppError::BadRequest("invalid cursor".to_string()))?
        .parse::<i64>()
        .map_err(|_| AppError::BadRequest("invalid cursor".to_string()))?;
    let ts = chrono::DateTime::<chrono::Utc>::from_timestamp_nanos(nanos);
    Ok(Some((ts, id)))
}

fn normalize_limit(limit: i64, default_limit: i64, max_limit: i64) -> i64 {
    if limit <= 0 {
        default_limit
    } else if limit > max_limit {
        max_limit
    } else {
        limit
    }
}

const MAX_WORKSPACE_PREVIEW_BYTES: usize = 256 * 1024;
const MAX_WORKSPACE_BINARY_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
const MAX_WORKSPACE_READ_BYTES: usize = 64 * 1024 * 1024;

fn thread_workspace_storage_prefix(deployment_id: i64, thread_id: i64) -> String {
    format!("{}/persistent/{}/workspace/", deployment_id, thread_id)
}

fn thread_uploads_storage_prefix(deployment_id: i64, thread_id: i64) -> String {
    format!("{}/persistent/{}/uploads/", deployment_id, thread_id)
}

fn task_workspace_storage_prefix(deployment_id: i64, project_id: i64, task_key: &str) -> String {
    format!("{}/{}/tasks/{}/", deployment_id, project_id, task_key)
}

fn sanitize_optional_relative_path(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    sanitize_relative_path(trimmed)
}

fn sanitize_relative_path(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("path query parameter is required".to_string()));
    }
    let normalized = trimmed.replace('\\', "/");
    let mut segments = Vec::new();
    for segment in normalized.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            return Err(AppError::BadRequest("invalid file path".to_string()));
        }
        segments.push(segment);
    }
    let cleaned = segments.join("/");
    if cleaned.is_empty() {
        return Err(AppError::BadRequest("invalid file path".to_string()));
    }
    Ok(cleaned)
}

fn build_escaped_like_query(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }
    let escaped = trimmed
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
        .to_lowercase();
    Some(format!("%{}%", escaped))
}

pub fn sanitize_download_filename(path: &str) -> String {
    let raw = Path::new(path)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("file");
    let cleaned: String = raw
        .chars()
        .filter(|ch| !ch.is_control())
        .map(|ch| match ch {
            '"' | '\'' | ';' | '\\' | '\r' | '\n' => '_',
            _ => ch,
        })
        .collect();
    if cleaned.trim().is_empty() {
        "file".to_string()
    } else {
        cleaned
    }
}

fn is_text_like(path: &str, mime_type: &str, body: &[u8]) -> bool {
    if !std::str::from_utf8(body).is_ok() {
        return false;
    }
    mime_type.starts_with("text/")
        || mime_type.contains("json")
        || mime_type.contains("xml")
        || mime_type.contains("yaml")
        || mime_type.contains("javascript")
        || mime_type.contains("typescript")
        || Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| matches!(ext, "txt" | "md" | "json" | "js" | "ts" | "tsx" | "jsx" | "py" | "rs" | "go" | "yml" | "yaml" | "xml" | "html" | "css" | "sql" | "toml"))
            .unwrap_or(false)
}

fn parse_conversation_message_type(value: &str) -> Result<models::ConversationMessageType, AppError> {
    match value {
        "user_message" => Ok(models::ConversationMessageType::UserMessage),
        "steer" => Ok(models::ConversationMessageType::Steer),
        "tool_result" => Ok(models::ConversationMessageType::ToolResult),
        "system_decision" => Ok(models::ConversationMessageType::SystemDecision),
        "approval_request" => Ok(models::ConversationMessageType::ApprovalRequest),
        "approval_response" => Ok(models::ConversationMessageType::ApprovalResponse),
        "execution_summary" => Ok(models::ConversationMessageType::ExecutionSummary),
        other => Err(AppError::Internal(format!(
            "Unknown conversation message_type '{}'",
            other
        ))),
    }
}

async fn list_workspace_directory(
    app_state: &AppState,
    deployment_id: i64,
    base_prefix: String,
    relative_path: String,
) -> Result<TaskWorkspaceListing, AppError> {
    let deps = common::deps::from_app(app_state).db().enc();
    let storage = commands::ResolveDeploymentStorageCommand::new(deployment_id)
        .execute_with_deps(&deps)
        .await?;

    let cleaned_relative_path = sanitize_optional_relative_path(&relative_path)?;
    let base_key_prefix = storage.object_key(&base_prefix);
    let target_prefix = if cleaned_relative_path.is_empty() {
        base_prefix
    } else {
        format!("{}{}/", base_prefix, cleaned_relative_path)
    };
    let target_key_prefix = storage.object_key(&target_prefix);

    let mut files = Vec::new();
    let mut continuation: Option<String> = None;

    loop {
        let mut request = storage
            .client()
            .list_objects_v2()
            .bucket(storage.bucket())
            .prefix(&target_key_prefix)
            .delimiter("/");
        if let Some(token) = continuation.as_deref() {
            request = request.continuation_token(token);
        }
        let response = request
            .send()
            .await
            .map_err(|e| AppError::S3(e.to_string()))?;

        for prefix in response.common_prefixes() {
            if let Some(pref) = prefix.prefix() {
                let relative = pref
                    .strip_prefix(&base_key_prefix)
                    .unwrap_or(pref)
                    .trim_end_matches('/')
                    .to_string();
                if !relative.is_empty() {
                    files.push(TaskWorkspaceFileEntry {
                        name: Path::new(&relative)
                            .file_name()
                            .and_then(|v| v.to_str())
                            .unwrap_or(&relative)
                            .to_string(),
                        path: relative,
                        is_dir: true,
                        size_bytes: None,
                        modified_at: None,
                    });
                }
            }
        }

        for object in response.contents() {
            let Some(key) = object.key() else { continue };
            if key == target_key_prefix || key.ends_with('/') {
                continue;
            }
            let relative = key.strip_prefix(&base_key_prefix).unwrap_or(key).to_string();
            if relative.is_empty() {
                continue;
            }
            files.push(TaskWorkspaceFileEntry {
                name: Path::new(&relative)
                    .file_name()
                    .and_then(|v| v.to_str())
                    .unwrap_or(&relative)
                    .to_string(),
                path: relative,
                is_dir: false,
                size_bytes: object.size().map(|v| v.max(0) as u64),
                modified_at: object.last_modified().map(|v| chrono::DateTime::from_timestamp(v.secs(), 0).unwrap_or_else(Utc::now)),
            });
        }

        if response.is_truncated().unwrap_or(false) {
            continuation = response.next_continuation_token().map(ToOwned::to_owned);
        } else {
            break;
        }
    }

    files.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    Ok(TaskWorkspaceListing {
        exists: cleaned_relative_path.is_empty() || !files.is_empty(),
        files,
    })
}

async fn read_workspace_file(
    app_state: &AppState,
    deployment_id: i64,
    base_prefix: String,
    relative_path: String,
) -> Result<(Vec<u8>, String), AppError> {
    let deps = common::deps::from_app(app_state).db().enc();
    let storage = commands::ResolveDeploymentStorageCommand::new(deployment_id)
        .execute_with_deps(&deps)
        .await?;
    let cleaned = sanitize_relative_path(&relative_path)?;
    let key = storage.object_key(&(base_prefix + &cleaned));
    let result = storage
        .client()
        .get_object()
        .bucket(storage.bucket())
        .key(&key)
        .send()
        .await
        .map_err(|e| AppError::S3(e.to_string()))?;
    let mime_type = result
        .content_type()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let body = result
        .body
        .collect()
        .await
        .map_err(|e| AppError::S3(e.to_string()))?
        .into_bytes()
        .to_vec();
    if body.len() > MAX_WORKSPACE_READ_BYTES {
        return Err(AppError::BadRequest("file too large".to_string()));
    }
    Ok((body, mime_type))
}

pub async fn search_actor_projects(
    app_state: &AppState,
    deployment_id: i64,
    actor_id: i64,
    query: String,
    limit: i64,
    cursor: Option<String>,
) -> Result<CursorPage<ActorProject>, AppError> {
    let limit = normalize_limit(limit, 20, 100);
    let cursor = cursor
        .as_deref()
        .map(decode_time_id_cursor)
        .transpose()?
        .flatten();
    let like = build_escaped_like_query(&query);

    let rows: Vec<ActorProject> = sqlx::query_as!(
        ActorProject,
        r#"
        SELECT id, deployment_id, actor_id, name, description, status, coordinator_thread_id,
               review_thread_id, metadata, created_at, updated_at, archived_at
        FROM actor_projects
        WHERE deployment_id = $1
          AND actor_id = $2
          AND archived_at IS NULL
          AND ($3::text IS NULL OR LOWER(name) LIKE $3 ESCAPE '\')
          AND (
            $4::timestamptz IS NULL OR $5::bigint IS NULL
            OR updated_at < $4
            OR (updated_at = $4 AND id < $5)
          )
        ORDER BY updated_at DESC, id DESC
        LIMIT $6
        "#,
        deployment_id,
        actor_id,
        like,
        cursor.map(|v| v.0),
        cursor.map(|v| v.1),
        limit + 1,
    )
    .fetch_all(app_state.db_router.reader(ReadConsistency::Eventual))
    .await?;

    let has_more = rows.len() as i64 > limit;
    let mut data = rows;
    if has_more {
        data.truncate(limit as usize);
    }
    let next_cursor = data
        .last()
        .filter(|_| has_more)
        .map(|last| encode_time_id_cursor(last.updated_at, last.id));

    Ok(CursorPage {
        data,
        limit,
        has_more,
        next_cursor,
    })
}

pub async fn create_actor_project(
    app_state: &AppState,
    deployment_id: i64,
    actor_id: i64,
    request: CreateActorProjectRequest,
) -> Result<ActorProject, AppError> {
    get_actor_by_id(app_state, deployment_id, actor_id).await?;
    let selected_agent_id = request.agent_id;

    let mut command = CreateActorProjectCommand::new(
        app_state.sf.next_id()? as i64,
        deployment_id,
        actor_id,
        request.name,
        request.status.unwrap_or_else(|| "active".to_string()),
    );

    if let Some(description) = request.description {
        command = command.with_description(description);
    }
    if let Some(metadata) = request.metadata {
        command = command.with_metadata(metadata);
    }

    let project = command.execute_with_db(app_state.db_router.writer()).await?;

    if let Some(agent_id) = selected_agent_id {
        if let Some(coordinator_thread_id) = project.coordinator_thread_id {
            UpsertThreadAgentAssignmentCommand::new(coordinator_thread_id, agent_id)
                .execute_with_db(app_state.db_router.writer())
                .await?;
        }
        if let Some(review_thread_id) = project.review_thread_id {
            UpsertThreadAgentAssignmentCommand::new(review_thread_id, agent_id)
                .execute_with_db(app_state.db_router.writer())
                .await?;
        }
    }

    Ok(project)
}

pub async fn get_actor_project_by_id(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
) -> Result<ActorProject, AppError> {
    GetActorProjectByIdQuery::new(project_id, deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?
        .ok_or_else(|| AppError::NotFound("Project not found".to_string()))
}

pub async fn list_agent_threads(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
    include_archived: bool,
) -> Result<Vec<AgentThread>, AppError> {
    let mut query = ListAgentThreadsQuery::new(deployment_id, project_id);
    if include_archived {
        query = query.include_archived();
    }

    query
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn create_agent_thread(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
    request: CreateAgentThreadRequest,
) -> Result<AgentThread, AppError> {
    let project = get_actor_project_by_id(app_state, deployment_id, project_id).await?;
    let thread_id = app_state.sf.next_id()? as i64;
    let CreateAgentThreadRequest {
        title,
        agent_id,
        system_instructions,
        thread_purpose,
        responsibility,
        reusable,
        accepts_assignments,
        capability_tags,
        metadata,
    } = request;

    let resolved_thread_purpose =
        thread_purpose.unwrap_or_else(|| models::agent_thread::purpose::CONVERSATION.to_string());
    if !matches!(
        resolved_thread_purpose.as_str(),
        models::agent_thread::purpose::CONVERSATION
            | models::agent_thread::purpose::COORDINATOR
            | models::agent_thread::purpose::EXECUTION
            | models::agent_thread::purpose::REVIEW
    ) {
        return Err(AppError::Validation("invalid thread_purpose".to_string()));
    }
    let resolved_responsibility = responsibility.filter(|value| !value.trim().is_empty());
    let resolved_capability_tags = capability_tags.unwrap_or_default();
    let resolved_reusable = reusable.unwrap_or(false);
    let resolved_accepts_assignments = accepts_assignments.unwrap_or(false);
    let generated_system_instructions = build_default_thread_system_instructions(
        &title,
        &resolved_thread_purpose,
        resolved_responsibility.as_deref(),
        resolved_reusable,
        resolved_accepts_assignments,
        &resolved_capability_tags,
    );

    let mut create_thread = CreateAgentThreadCommand::new(
        thread_id,
        deployment_id,
        project.actor_id,
        project.id,
        title,
        resolved_thread_purpose.clone(),
        "idle".to_string(),
    );
    create_thread = create_thread
        .with_thread_purpose(resolved_thread_purpose)
        .with_capability_tags(resolved_capability_tags);
    if let Some(responsibility) = resolved_responsibility {
        create_thread = create_thread.with_responsibility(responsibility);
    }
    if resolved_reusable {
        create_thread = create_thread.mark_reusable();
    }
    if resolved_accepts_assignments {
        create_thread = create_thread.allow_assignments();
    }
    create_thread = create_thread
        .with_system_instructions(system_instructions.unwrap_or(generated_system_instructions));
    if let Some(metadata) = metadata {
        create_thread = create_thread.with_metadata(metadata);
    }
    let thread = create_thread
        .execute_with_db(app_state.db_router.writer())
        .await?;

    if let Some(agent_id) = agent_id {
        UpsertThreadAgentAssignmentCommand::new(thread.id, agent_id)
            .execute_with_db(app_state.db_router.writer())
            .await?;
    }

    get_agent_thread_by_id(app_state, deployment_id, thread_id).await
}

fn build_default_thread_system_instructions(
    title: &str,
    thread_purpose: &str,
    responsibility: Option<&str>,
    reusable: bool,
    accepts_assignments: bool,
    capability_tags: &[String],
) -> String {
    let mut lines = vec![format!(
        "You are the '{}' thread. Operate as a stable {} work lane.",
        title, thread_purpose
    )];

    lines.push(format!(
        "Thread purpose: {}. Visibility: {}.",
        thread_purpose,
        if thread_purpose == models::agent_thread::purpose::CONVERSATION {
            "user_facing"
        } else {
            "internal"
        }
    ));

    if let Some(responsibility) = responsibility {
        lines.push(format!(
            "Primary responsibility: {}. Default to that specialization unless the active assignment clearly narrows the scope further.",
            responsibility
        ));
    }

    if !capability_tags.is_empty() {
        lines.push(format!(
            "Primary capabilities: {}. Prefer solutions and reviews that stay inside these strengths.",
            capability_tags.join(", ")
        ));
    }

    match thread_purpose {
        models::agent_thread::purpose::COORDINATOR => {
            lines.push(
                "Act as the coordinator for this project lane. Route work, decide next steps, and own task completion decisions instead of absorbing delegated execution yourself."
                    .to_string(),
            );
        }
        models::agent_thread::purpose::EXECUTION => {
            lines.push(
                "Treat this thread as an execution lane. Work from assigned board items and handoff files first, not from broad conversation reinterpretation."
                    .to_string(),
            );
        }
        models::agent_thread::purpose::REVIEW => {
            lines.push(
                "Default to evidence-first review behavior. Validate claims, call out gaps directly, and keep findings concrete."
                    .to_string(),
            );
        }
        _ => {
            lines.push(
                "Keep work aligned to the active thread event, board item, assignment chain, and task graph before falling back to broader conversation history."
                    .to_string(),
            );
        }
    }

    if reusable {
        lines.push(
            "Preserve continuity in task state and `/workspace/` handoff files so this thread remains reusable across tasks."
                .to_string(),
        );
    }

    if accepts_assignments {
        lines.push(
            "When assignments exist, treat the assignment queue as the workload source of truth. Complete the active assignment cleanly before expanding scope."
                .to_string(),
        );
    }

    lines.push(
        "For substantial work, keep the project task board current, use the thread task graph for dependent execution steps, and prefer `/workspace/` files for briefs, planning, and handoffs."
            .to_string(),
    );

    lines.join("\n")
}

pub async fn get_agent_thread_by_id(
    app_state: &AppState,
    deployment_id: i64,
    thread_id: i64,
) -> Result<AgentThread, AppError> {
    GetAgentThreadByIdQuery::new(thread_id, deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".to_string()))
}

pub async fn search_actor_project_threads(
    app_state: &AppState,
    deployment_id: i64,
    actor_id: i64,
    query: String,
    limit: i64,
    cursor: Option<String>,
) -> Result<CursorPage<AgentThread>, AppError> {
    let limit = normalize_limit(limit, 20, 100);
    let cursor = cursor
        .as_deref()
        .map(decode_time_id_cursor)
        .transpose()?
        .flatten();
    let like = build_escaped_like_query(&query);

    let rows: Vec<AgentThread> = sqlx::query_as!(
        AgentThread,
        r#"
        SELECT id, deployment_id, actor_id, project_id,
               title, thread_purpose as "thread_kind!", CASE WHEN thread_purpose = 'conversation' THEN 'user_facing' ELSE 'internal' END as "thread_visibility!",
               thread_purpose, responsibility,
               reusable, accepts_assignments, capability_tags, status, system_instructions, last_activity_at, completed_at,
               execution_state, next_event_sequence, metadata, created_at, updated_at, archived_at
        FROM agent_threads
        WHERE deployment_id = $1
          AND actor_id = $2
          AND archived_at IS NULL
          AND ($3::text IS NULL OR LOWER(title) LIKE $3 ESCAPE '\')
          AND (
            $4::timestamptz IS NULL OR $5::bigint IS NULL
            OR last_activity_at < $4
            OR (last_activity_at = $4 AND id < $5)
          )
        ORDER BY last_activity_at DESC, id DESC
        LIMIT $6
        "#,
        deployment_id,
        actor_id,
        like,
        cursor.map(|v| v.0),
        cursor.map(|v| v.1),
        limit + 1,
    )
    .fetch_all(app_state.db_router.reader(ReadConsistency::Eventual))
    .await?;

    let has_more = rows.len() as i64 > limit;
    let mut data = rows;
    if has_more {
        data.truncate(limit as usize);
    }
    let next_cursor = data
        .last()
        .filter(|_| has_more)
        .map(|last| encode_time_id_cursor(last.last_activity_at, last.id));

    Ok(CursorPage {
        data,
        limit,
        has_more,
        next_cursor,
    })
}

pub async fn update_actor_project(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
    request: UpdateActorProjectRequest,
) -> Result<ActorProject, AppError> {
    let existing = get_actor_project_by_id(app_state, deployment_id, project_id).await?;
    let name = request.name.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let description = request.description.map(|v| v.trim().to_string());
    let status = request.status.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());

    if name.is_none() && description.is_none() && status.is_none() {
        return Ok(existing);
    }

    let updated = sqlx::query_as!(
        ActorProject,
        r#"
        UPDATE actor_projects
        SET
            name = COALESCE($3, name),
            description = COALESCE($4, description),
            status = COALESCE($5, status),
            updated_at = NOW()
        WHERE id = $1 AND deployment_id = $2
        RETURNING id, deployment_id, actor_id, name, description, status, coordinator_thread_id,
                  review_thread_id, metadata, created_at, updated_at, archived_at
        "#,
        project_id,
        deployment_id,
        name,
        description,
        status,
    )
    .fetch_one(app_state.db_router.writer())
    .await?;
    Ok(updated)
}

pub async fn set_actor_project_archived(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
    archived: bool,
) -> Result<ActorProject, AppError> {
    get_actor_project_by_id(app_state, deployment_id, project_id).await?;
    let updated = sqlx::query_as!(
        ActorProject,
        r#"
        UPDATE actor_projects
        SET archived_at = CASE WHEN $3 THEN NOW() ELSE NULL END, updated_at = NOW()
        WHERE id = $1 AND deployment_id = $2
        RETURNING id, deployment_id, actor_id, name, description, status, coordinator_thread_id,
                  review_thread_id, metadata, created_at, updated_at, archived_at
        "#,
        project_id,
        deployment_id,
        archived,
    )
    .fetch_one(app_state.db_router.writer())
    .await?;
    Ok(updated)
}

pub async fn update_agent_thread(
    app_state: &AppState,
    deployment_id: i64,
    thread_id: i64,
    request: UpdateAgentThreadRequest,
) -> Result<AgentThread, AppError> {
    let existing = get_agent_thread_by_id(app_state, deployment_id, thread_id).await?;
    let UpdateAgentThreadRequest {
        title,
        agent_id,
        system_instructions,
    } = request;

    let has_title = title.is_some();
    let has_agent_id = agent_id.is_some();
    let has_system_instructions = system_instructions.is_some();

    if !has_agent_id && !has_title && !has_system_instructions {
        return Ok(existing);
    }

    let mut command = commands::UpdateAgentThreadCommand::new(thread_id, deployment_id);

    if let Some(title) = title {
        command = command.with_title(title.trim().to_string());
    }
    if let Some(system_instructions) = system_instructions {
        command = command.with_system_instructions(Some(system_instructions));
    }
    let thread = command.execute_with_db(app_state.db_router.writer()).await?;

    if let Some(agent_id) = agent_id {
        UpsertThreadAgentAssignmentCommand::new(thread.id, agent_id)
            .execute_with_db(app_state.db_router.writer())
            .await?;
    }

    get_agent_thread_by_id(app_state, deployment_id, thread_id).await
}

pub async fn set_agent_thread_archived(
    app_state: &AppState,
    deployment_id: i64,
    thread_id: i64,
    archived: bool,
) -> Result<AgentThread, AppError> {
    let thread = get_agent_thread_by_id(app_state, deployment_id, thread_id).await?;
    if archived
        && matches!(
            thread.thread_purpose.as_str(),
            models::agent_thread::purpose::EXECUTION | models::agent_thread::purpose::REVIEW
        )
    {
        return Err(AppError::BadRequest(
            "Execution and review threads cannot be archived".to_string(),
        ));
    }

    let updated = sqlx::query_as!(
        AgentThread,
        r#"
        UPDATE agent_threads
        SET archived_at = CASE WHEN $3 THEN NOW() ELSE NULL END, updated_at = NOW()
        WHERE id = $1 AND deployment_id = $2
        RETURNING id, deployment_id, actor_id, project_id,
                  title, thread_purpose as "thread_kind!", CASE WHEN thread_purpose = 'conversation' THEN 'user_facing' ELSE 'internal' END as "thread_visibility!",
                  thread_purpose, responsibility,
                  reusable, accepts_assignments, capability_tags, status, system_instructions, last_activity_at, completed_at,
                  execution_state, next_event_sequence, metadata, created_at, updated_at, archived_at
        "#,
        thread_id,
        deployment_id,
        archived,
    )
    .fetch_one(app_state.db_router.writer())
    .await?;
    Ok(updated)
}

pub async fn list_thread_messages(
    app_state: &AppState,
    deployment_id: i64,
    thread_id: i64,
    limit: i64,
    before_id: Option<i64>,
    after_id: Option<i64>,
) -> Result<(Vec<ConversationRecord>, bool), AppError> {
    get_agent_thread_by_id(app_state, deployment_id, thread_id).await?;
    let limit = normalize_limit(limit, 50, 100);
    let rows = sqlx::query!(
        r#"
        SELECT id, thread_id, execution_run_id, timestamp, content, message_type, created_at, updated_at, metadata
        FROM conversations
        WHERE thread_id = $1
          AND ($2::bigint IS NULL OR id < $2)
          AND ($3::bigint IS NULL OR id > $3)
        ORDER BY
          CASE WHEN $3::bigint IS NOT NULL THEN id END ASC,
          CASE WHEN $3::bigint IS NULL THEN id END DESC
        LIMIT $4
        "#,
        thread_id,
        before_id,
        after_id,
        limit + 1,
    )
    .fetch_all(app_state.db_router.reader(ReadConsistency::Eventual))
    .await?;

    let has_more = rows.len() as i64 > limit;
    let mut data: Vec<ConversationRecord> = rows
        .into_iter()
        .map(|row| {
            Ok(ConversationRecord {
                id: row.id,
                thread_id: row.thread_id,
                execution_run_id: row.execution_run_id,
                timestamp: row.timestamp,
                content: serde_json::from_value(row.content).map_err(|e| {
                    AppError::Internal(format!(
                        "Failed to deserialize conversation content: {}",
                        e
                    ))
                })?,
                message_type: parse_conversation_message_type(&row.message_type)?,
                created_at: row.created_at,
                updated_at: row.updated_at,
                metadata: row.metadata,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    if has_more {
        data.truncate(limit as usize);
    }
    Ok((data, has_more))
}

pub async fn list_thread_filesystem(
    app_state: &AppState,
    deployment_id: i64,
    thread_id: i64,
    path: String,
) -> Result<TaskWorkspaceListing, AppError> {
    get_agent_thread_by_id(app_state, deployment_id, thread_id).await?;
    let cleaned = sanitize_optional_relative_path(&path)?;
    if cleaned.is_empty() {
        return Ok(TaskWorkspaceListing {
            exists: true,
            files: vec![
                TaskWorkspaceFileEntry {
                    path: "uploads".to_string(),
                    name: "uploads".to_string(),
                    is_dir: true,
                    size_bytes: None,
                    modified_at: None,
                },
                TaskWorkspaceFileEntry {
                    path: "workspace".to_string(),
                    name: "workspace".to_string(),
                    is_dir: true,
                    size_bytes: None,
                    modified_at: None,
                },
            ],
        });
    }
    match cleaned.as_str() {
        "workspace" => {
            list_workspace_directory(
                app_state,
                deployment_id,
                thread_workspace_storage_prefix(deployment_id, thread_id),
                String::new(),
            )
            .await
        }
        "uploads" => {
            list_workspace_directory(
                app_state,
                deployment_id,
                thread_uploads_storage_prefix(deployment_id, thread_id),
                String::new(),
            )
            .await
        }
        _ if cleaned.starts_with("workspace/") => {
            list_workspace_directory(
                app_state,
                deployment_id,
                thread_workspace_storage_prefix(deployment_id, thread_id),
                cleaned.trim_start_matches("workspace/").to_string(),
            )
            .await
        }
        _ if cleaned.starts_with("uploads/") => {
            list_workspace_directory(
                app_state,
                deployment_id,
                thread_uploads_storage_prefix(deployment_id, thread_id),
                cleaned.trim_start_matches("uploads/").to_string(),
            )
            .await
        }
        _ => Err(AppError::BadRequest(
            "path must be inside workspace or uploads".to_string(),
        )),
    }
}

pub async fn get_thread_filesystem_file(
    app_state: &AppState,
    deployment_id: i64,
    thread_id: i64,
    path: String,
) -> Result<(Vec<u8>, String, String), AppError> {
    get_agent_thread_by_id(app_state, deployment_id, thread_id).await?;
    let cleaned = sanitize_relative_path(&path)?;
    let (base_prefix, relative) = if cleaned == "workspace" || cleaned == "uploads" {
        return Err(AppError::BadRequest("requested path is a directory".to_string()));
    } else if cleaned.starts_with("workspace/") {
        (
            thread_workspace_storage_prefix(deployment_id, thread_id),
            cleaned.trim_start_matches("workspace/").to_string(),
        )
    } else if cleaned.starts_with("uploads/") {
        (
            thread_uploads_storage_prefix(deployment_id, thread_id),
            cleaned.trim_start_matches("uploads/").to_string(),
        )
    } else {
        return Err(AppError::BadRequest(
            "path must be inside workspace or uploads".to_string(),
        ));
    };
    let (body, mime_type) = read_workspace_file(app_state, deployment_id, base_prefix, relative).await?;
    Ok((body, mime_type, cleaned))
}

pub async fn list_actor_mcp_servers(
    app_state: &AppState,
    deployment_id: i64,
    actor_id: i64,
) -> Result<Vec<ActorMcpServerSummary>, AppError> {
    get_actor_by_id(app_state, deployment_id, actor_id).await?;
    let servers = GetMcpServersQuery::new(deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;
    let connections = sqlx::query!(
        r#"
        SELECT mcp_server_id, connection_metadata as "connection_metadata!: serde_json::Value"
        FROM actor_mcp_server_connections
        WHERE deployment_id = $1 AND actor_id = $2
        "#,
        deployment_id,
        actor_id,
    )
    .fetch_all(app_state.db_router.reader(ReadConsistency::Eventual))
    .await?;

    let by_server: std::collections::HashMap<i64, models::McpConnectionMetadata> = connections
        .into_iter()
        .filter_map(|row| {
            serde_json::from_value::<models::McpConnectionMetadata>(row.connection_metadata)
                .ok()
                .map(|meta| (row.mcp_server_id, meta))
        })
        .collect();

    let now = Utc::now();
    let result = servers
        .into_iter()
        .map(|server| {
            let auth_type = match server.config.auth.as_ref() {
                Some(models::McpAuthConfig::Token { .. }) => "token".to_string(),
                Some(models::McpAuthConfig::OAuthClientCredentials { .. }) => {
                    "oauth_client_credentials".to_string()
                }
                Some(models::McpAuthConfig::OAuthAuthorizationCodePublicPkce { .. }) => {
                    "oauth_authorization_code_public_pkce".to_string()
                }
                Some(models::McpAuthConfig::OAuthAuthorizationCodeConfidentialPkce { .. }) => {
                    "oauth_authorization_code_confidential_pkce".to_string()
                }
                None => "none".to_string(),
            };
            let requires_user_connection = server
                .config
                .auth
                .as_ref()
                .map(|auth| auth.requires_user_connection())
                .unwrap_or(false);
            let mut summary = ActorMcpServerSummary {
                id: server.id,
                name: server.name,
                endpoint: server.config.endpoint,
                auth_type,
                requires_user_connection,
                connection_status: "ready".to_string(),
                connected_at: None,
                expires_at: None,
            };
            if requires_user_connection {
                summary.connection_status = "not_connected".to_string();
                if let Some(metadata) = by_server.get(&summary.id) {
                    summary.connected_at = metadata.connected_at;
                    summary.expires_at = metadata.expires_at;
                    summary.connection_status = if metadata.expires_at.is_some_and(|v| v < now) {
                        "expired".to_string()
                    } else {
                        "connected".to_string()
                    };
                }
            }
            summary
        })
        .collect();
    Ok(result)
}

fn generate_random_base64_url(size: usize) -> Result<String, AppError> {
    use base64::Engine;
    use rand::RngCore;
    let mut bytes = vec![0u8; size];
    rand::rng().fill_bytes(&mut bytes);
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

fn compute_code_challenge(verifier: &str) -> String {
    use base64::Engine;
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

pub async fn build_actor_mcp_server_connect_url(
    app_state: &AppState,
    deployment_id: i64,
    actor_id: i64,
    mcp_server_id: i64,
) -> Result<ActorMcpServerConnectResponse, AppError> {
    get_actor_by_id(app_state, deployment_id, actor_id).await?;
    let server = GetMcpServerByIdQuery::new(deployment_id, mcp_server_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;
    let auth = server
        .config
        .auth
        .ok_or_else(|| AppError::BadRequest("This MCP server does not require actor consent".to_string()))?;

    let (client_id, auth_url, token_url, scopes, resource) = match auth {
        models::McpAuthConfig::OAuthAuthorizationCodePublicPkce {
            client_id,
            auth_url,
            token_url,
            scopes,
            resource,
            ..
        } => (
            client_id.ok_or_else(|| AppError::BadRequest("MCP server client_id is missing".to_string()))?,
            auth_url.ok_or_else(|| AppError::BadRequest("MCP server auth_url is missing".to_string()))?,
            token_url.ok_or_else(|| AppError::BadRequest("MCP server token_url is missing".to_string()))?,
            scopes,
            resource,
        ),
        models::McpAuthConfig::OAuthAuthorizationCodeConfidentialPkce {
            client_id,
            auth_url,
            token_url,
            scopes,
            resource,
            ..
        } => (
            client_id,
            auth_url.ok_or_else(|| AppError::BadRequest("MCP server auth_url is missing".to_string()))?,
            token_url.ok_or_else(|| AppError::BadRequest("MCP server token_url is missing".to_string()))?,
            scopes,
            resource,
        ),
        _ => {
            return Err(AppError::BadRequest(
                "This MCP server does not require actor consent".to_string(),
            ))
        }
    };

    let state = generate_random_base64_url(24)?;
    let code_verifier = generate_random_base64_url(32)?;
    let redirect_uri = "https://agentlink.wacht.services/service/mcp/consent/callback".to_string();
    sqlx::query!(
        r#"
        INSERT INTO mcp_oauth_states (
            state, deployment_id, actor_id, mcp_server_id, code_verifier, client_id, token_url, redirect_uri, resource, expires_at, created_at, updated_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,NOW(),NOW())
        "#,
        &state,
        deployment_id,
        actor_id,
        mcp_server_id,
        &code_verifier,
        &client_id,
        &token_url,
        &redirect_uri,
        resource.clone(),
        Utc::now() + chrono::Duration::minutes(15),
    )
    .execute(app_state.db_router.writer())
    .await?;

    let mut url = url::Url::parse(&auth_url)
        .map_err(|_| AppError::BadRequest("invalid auth_url".to_string()))?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("response_type", "code");
        pairs.append_pair("client_id", &client_id);
        pairs.append_pair("redirect_uri", &redirect_uri);
        pairs.append_pair("state", &state);
        pairs.append_pair("code_challenge", &compute_code_challenge(&code_verifier));
        pairs.append_pair("code_challenge_method", "S256");
        if !scopes.is_empty() {
            pairs.append_pair("scope", &scopes.join(" "));
        }
        if let Some(resource) = resource.as_deref().filter(|v| !v.trim().is_empty()) {
            pairs.append_pair("resource", resource);
        }
    }
    Ok(ActorMcpServerConnectResponse {
        auth_url: url.to_string(),
    })
}

pub async fn disconnect_actor_mcp_server(
    app_state: &AppState,
    deployment_id: i64,
    actor_id: i64,
    mcp_server_id: i64,
) -> Result<(), AppError> {
    get_actor_by_id(app_state, deployment_id, actor_id).await?;
    sqlx::query!(
        r#"
        DELETE FROM actor_mcp_server_connections
        WHERE deployment_id = $1 AND actor_id = $2 AND mcp_server_id = $3
        "#,
        deployment_id,
        actor_id,
        mcp_server_id,
    )
    .execute(app_state.db_router.writer())
    .await?;
    Ok(())
}

pub async fn execute_agent_thread_async(
    app_state: &AppState,
    deployment_id: i64,
    thread_id: i64,
    request: ExecuteAgentRequest,
) -> Result<ExecuteAgentResponse, AppError> {
    get_agent_thread_by_id(app_state, deployment_id, thread_id).await?;
    agent_thread_execution_app::execute_agent_async(app_state, deployment_id, thread_id, request)
        .await
}

pub async fn get_project_task_board_by_project_id(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
) -> Result<ProjectTaskBoard, AppError> {
    get_actor_project_by_id(app_state, deployment_id, project_id).await?;
    GetProjectTaskBoardByProjectIdQuery::new(project_id, deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?
        .ok_or_else(|| AppError::NotFound("Project task board not found".to_string()))
}

pub async fn list_project_task_board_items(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
) -> Result<Vec<ProjectTaskBoardItem>, AppError> {
    let board = get_project_task_board_by_project_id(app_state, deployment_id, project_id).await?;
    ListProjectTaskBoardItemsQuery::new(board.id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn create_project_task_board_item(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
    request: CreateProjectTaskBoardItemRequest,
) -> Result<ProjectTaskBoardItem, AppError> {
    let project = get_actor_project_by_id(app_state, deployment_id, project_id).await?;
    let board = get_project_task_board_by_project_id(app_state, deployment_id, project_id).await?;
    let item_id = app_state.sf.next_id()? as i64;
    let task_key = format!("TASK-{}", item_id);
    let status = request.status.unwrap_or_else(|| "pending".to_string());
    let priority = request.priority.unwrap_or_else(|| "neutral".to_string());
    let assigned_thread_id = project.coordinator_thread_id;

    let item = CreateProjectTaskBoardItemCommand {
        id: item_id,
        board_id: board.id,
        task_key,
        title: request.title.trim().to_string(),
        description: request.description,
        status,
        priority,
        assigned_thread_id,
        metadata: serde_json::json!({}),
    }
    .execute_with_db(app_state.db_router.writer())
    .await?;

    CreateProjectTaskBoardItemEventCommand {
        id: app_state.sf.next_id()? as i64,
        board_item_id: item.id,
        thread_id: item.assigned_thread_id,
        execution_run_id: None,
        event_type: "task_created".to_string(),
        summary: "Task created".to_string(),
        body_markdown: None,
        details: serde_json::json!({
            "board_id": item.board_id,
            "task_key": item.task_key,
            "status": item.status,
            "assigned_thread_id": item.assigned_thread_id,
            "priority": item.priority,
        }),
    }
    .execute_with_db(app_state.db_router.writer())
    .await?;

    if let Some(coordinator_thread_id) = project.coordinator_thread_id {
        let payload = models::thread_event::TaskRoutingEventPayload {
            board_item_id: item.id,
        };
        DispatchThreadEventCommand::new(
            EnqueueThreadEventCommand::new(
                app_state.sf.next_id()? as i64,
                deployment_id,
                coordinator_thread_id,
                models::thread_event::event_type::TASK_ROUTING.to_string(),
            )
            .with_board_item_id(item.id)
            .with_priority(30)
            .with_payload(serde_json::to_value(payload).map_err(|err| {
                AppError::Internal(format!("Failed to serialize task routing payload: {}", err))
            })?),
        )
        .execute_with_deps(&common::deps::from_app(app_state).db().nats().id())
        .await?;
    }

    Ok(item)
}

pub async fn get_project_task_board_item_by_id(
    app_state: &AppState,
    deployment_id: i64,
    item_id: i64,
) -> Result<ProjectTaskBoardItem, AppError> {
    let item = GetProjectTaskBoardItemByIdQuery::new(item_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?
        .ok_or_else(|| AppError::NotFound("Project task board item not found".to_string()))?;

    match GetProjectTaskBoardByIdQuery::new(item.board_id, deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?
    {
        Some(_) => Ok(item),
        None => Err(AppError::NotFound(
            "Project task board item not found".to_string(),
        )),
    }
}

pub async fn list_project_task_board_item_events(
    app_state: &AppState,
    deployment_id: i64,
    item_id: i64,
) -> Result<Vec<ProjectTaskBoardItemEvent>, AppError> {
    get_project_task_board_item_by_id(app_state, deployment_id, item_id).await?;
    ListProjectTaskBoardItemEventsQuery::new(item_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn list_project_task_board_item_assignments(
    app_state: &AppState,
    deployment_id: i64,
    item_id: i64,
) -> Result<Vec<ProjectTaskBoardItemAssignment>, AppError> {
    get_project_task_board_item_by_id(app_state, deployment_id, item_id).await?;
    ListProjectTaskBoardItemAssignmentsQuery::new(item_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn list_project_task_board_item_relations(
    app_state: &AppState,
    deployment_id: i64,
    item_id: i64,
) -> Result<Vec<ProjectTaskBoardItemRelation>, AppError> {
    get_project_task_board_item_by_id(app_state, deployment_id, item_id).await?;
    ListProjectTaskBoardItemRelationsQuery::new(item_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn append_project_task_board_item_journal_entry(
    app_state: &AppState,
    deployment_id: i64,
    item_id: i64,
    summary: String,
    details: Option<String>,
    body_markdown: Option<String>,
    attachments: Option<serde_json::Value>,
) -> Result<ProjectTaskBoardItemEvent, AppError> {
    let item = get_project_task_board_item_by_id(app_state, deployment_id, item_id).await?;
    let summary = summary.trim().to_string();
    if summary.is_empty() {
        return Err(AppError::BadRequest(
            "summary must not be empty".to_string(),
        ));
    }

    let body_markdown = body_markdown
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .or_else(|| {
            details
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
        });

    let attachments = attachments
        .filter(|value| !value.is_null())
        .unwrap_or_else(|| serde_json::json!([]));

    let event = CreateProjectTaskBoardItemEventCommand {
        id: app_state.sf.next_id()? as i64,
        board_item_id: item.id,
        thread_id: None,
        execution_run_id: None,
        event_type: "task_journal_entry".to_string(),
        summary,
        body_markdown: body_markdown.clone(),
        details: serde_json::json!({
            "task_key": item.task_key,
            "details": body_markdown,
            "attachments": attachments,
        }),
    }
    .execute_with_db(app_state.db_router.writer())
    .await?;

    if matches!(item.status.as_str(), "blocked" | "needs_clarification") {
        let board = GetProjectTaskBoardByIdQuery::new(item.board_id, deployment_id)
            .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
            .await?
            .ok_or_else(|| AppError::NotFound("Project task board not found".to_string()))?;
        let project = GetActorProjectByIdQuery::new(board.project_id, deployment_id)
            .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
            .await?
            .ok_or_else(|| AppError::NotFound("Project not found".to_string()))?;

        if let Some(coordinator_thread_id) = project.coordinator_thread_id {
            let payload = models::thread_event::TaskRoutingEventPayload {
                board_item_id: item.id,
            };

            DispatchThreadEventCommand::new(
                EnqueueThreadEventCommand::new(
                    app_state.sf.next_id()? as i64,
                    deployment_id,
                    coordinator_thread_id,
                    models::thread_event::event_type::TASK_ROUTING.to_string(),
                )
                .with_board_item_id(item.id)
                .with_priority(15)
                .with_payload(serde_json::to_value(payload).map_err(|err| {
                    AppError::Internal(format!(
                        "Failed to serialize coordinator reroute payload: {}",
                        err
                    ))
                })?),
            )
            .execute_with_deps(&common::deps::from_app(app_state).db().nats().id())
            .await?;
        }
    }

    Ok(event)
}

pub async fn update_project_task_board_item(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
    item_id: i64,
    request: UpdateProjectTaskBoardItemRequest,
) -> Result<ProjectTaskBoardItem, AppError> {
    let item = get_project_task_board_item_by_id(app_state, deployment_id, item_id).await?;
    let board = get_project_task_board_by_project_id(app_state, deployment_id, project_id).await?;
    if item.board_id != board.id {
        return Err(AppError::NotFound("Project task board item not found".to_string()));
    }

    let title = request.title.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let description = request.description.map(|v| v.trim().to_string());

    if title.is_some() || description.is_some() {
        let updated = sqlx::query_as!(
            ProjectTaskBoardItem,
            r#"
            UPDATE project_task_board_items
            SET
                title = COALESCE($3, title),
                description = COALESCE($4, description),
                updated_at = NOW()
            WHERE id = $1 AND board_id = $2 AND archived_at IS NULL
            RETURNING id, board_id, task_key, title, description, status, priority,
                      assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at
            "#,
            item.id,
            board.id,
            title,
            description,
        )
        .fetch_one(app_state.db_router.writer())
        .await?;

        if request.status.is_none() && request.priority.is_none() {
            return Ok(updated);
        }
    }

    UpdateProjectTaskBoardItemCommand {
        board_id: board.id,
        task_key: item.task_key,
        status: request.status,
        priority: request.priority,
        metadata: item.metadata,
    }
    .execute_with_deps(&common::deps::from_app(app_state).db().nats().id())
    .await
}

pub async fn set_project_task_board_item_archived(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
    item_id: i64,
    archived: bool,
) -> Result<ProjectTaskBoardItem, AppError> {
    let item = get_project_task_board_item_by_id(app_state, deployment_id, item_id).await?;
    let board = get_project_task_board_by_project_id(app_state, deployment_id, project_id).await?;
    if item.board_id != board.id {
        return Err(AppError::NotFound("Project task board item not found".to_string()));
    }

    let updated = sqlx::query_as!(
        ProjectTaskBoardItem,
        r#"
        UPDATE project_task_board_items
        SET
            archived_at = CASE WHEN $3 THEN NOW() ELSE NULL END,
            updated_at = NOW()
        WHERE id = $1 AND board_id = $2
        RETURNING id, board_id, task_key, title, description, status, priority,
                  assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at
        "#,
        item_id,
        board.id,
        archived,
    )
    .fetch_one(app_state.db_router.writer())
    .await?;

    Ok(updated)
}

pub async fn list_project_task_board_item_filesystem(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
    item_id: i64,
    path: String,
) -> Result<TaskWorkspaceListing, AppError> {
    let item = get_project_task_board_item_by_id(app_state, deployment_id, item_id).await?;
    let board = get_project_task_board_by_project_id(app_state, deployment_id, project_id).await?;
    if item.board_id != board.id {
        return Err(AppError::NotFound("Project task board item not found".to_string()));
    }
    list_workspace_directory(
        app_state,
        deployment_id,
        task_workspace_storage_prefix(deployment_id, project_id, &item.task_key),
        path,
    )
    .await
}

pub async fn get_project_task_board_item_filesystem_file(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
    item_id: i64,
    path: String,
) -> Result<TaskWorkspaceFileContent, AppError> {
    let item = get_project_task_board_item_by_id(app_state, deployment_id, item_id).await?;
    let board = get_project_task_board_by_project_id(app_state, deployment_id, project_id).await?;
    if item.board_id != board.id {
        return Err(AppError::NotFound("Project task board item not found".to_string()));
    }
    let (body, mime_type) = read_workspace_file(
        app_state,
        deployment_id,
        task_workspace_storage_prefix(deployment_id, project_id, &item.task_key),
        path.clone(),
    )
    .await?;
    let size_bytes = body.len() as u64;
    let mut preview = body.as_slice();
    let mut truncated = false;
    if preview.len() > MAX_WORKSPACE_PREVIEW_BYTES {
        preview = &preview[..MAX_WORKSPACE_PREVIEW_BYTES];
        truncated = true;
    }
    let is_text = is_text_like(&path, &mime_type, preview);
    Ok(TaskWorkspaceFileContent {
        path: sanitize_relative_path(&path)?,
        name: Path::new(&path)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or(path.as_str())
            .to_string(),
        mime_type,
        is_text,
        size_bytes,
        truncated,
        content: if is_text {
            String::from_utf8_lossy(preview).to_string()
        } else {
            String::new()
        },
        content_base64: if !is_text && body.len() <= MAX_WORKSPACE_BINARY_PREVIEW_BYTES {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(body)
        } else {
            String::new()
        },
    })
}

pub async fn get_agent_thread_state(
    app_state: &AppState,
    deployment_id: i64,
    thread_id: i64,
) -> Result<AgentThreadState, AppError> {
    GetAgentThreadStateQuery::new(thread_id, deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn list_pending_thread_events(
    app_state: &AppState,
    deployment_id: i64,
    thread_id: i64,
) -> Result<Vec<ThreadEvent>, AppError> {
    get_agent_thread_by_id(app_state, deployment_id, thread_id).await?;
    ListPendingThreadEventsQuery::new(thread_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn get_thread_event_by_id(
    app_state: &AppState,
    deployment_id: i64,
    event_id: i64,
) -> Result<ThreadEvent, AppError> {
    let event = GetThreadEventByIdQuery::new(event_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?
        .ok_or_else(|| AppError::NotFound("Thread event not found".to_string()))?;

    if event.deployment_id != deployment_id {
        return Err(AppError::NotFound("Thread event not found".to_string()));
    }

    Ok(event)
}

pub async fn list_assignments_for_thread(
    app_state: &AppState,
    deployment_id: i64,
    thread_id: i64,
) -> Result<Vec<ProjectTaskBoardItemAssignment>, AppError> {
    get_agent_thread_by_id(app_state, deployment_id, thread_id).await?;
    ListAssignmentsForThreadQuery::new(thread_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn get_latest_thread_task_graph(
    app_state: &AppState,
    deployment_id: i64,
    thread_id: i64,
) -> Result<Option<ThreadTaskGraph>, AppError> {
    get_agent_thread_by_id(app_state, deployment_id, thread_id).await?;
    GetLatestThreadTaskGraphQuery::new(deployment_id, thread_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn get_thread_task_graph_by_id(
    app_state: &AppState,
    deployment_id: i64,
    graph_id: i64,
) -> Result<ThreadTaskGraph, AppError> {
    let graph = GetThreadTaskGraphByIdQuery::new(graph_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?
        .ok_or_else(|| AppError::NotFound("Thread task graph not found".to_string()))?;

    if graph.deployment_id != deployment_id {
        return Err(AppError::NotFound(
            "Thread task graph not found".to_string(),
        ));
    }

    Ok(graph)
}

pub async fn list_thread_task_nodes(
    app_state: &AppState,
    deployment_id: i64,
    graph_id: i64,
    include_terminal: bool,
) -> Result<Vec<ThreadTaskNode>, AppError> {
    get_thread_task_graph_by_id(app_state, deployment_id, graph_id).await?;
    let mut query = ListThreadTaskNodesQuery::new(graph_id);
    if !include_terminal {
        query = query.without_terminal();
    }
    query
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn list_thread_task_edges(
    app_state: &AppState,
    deployment_id: i64,
    graph_id: i64,
) -> Result<Vec<ThreadTaskEdge>, AppError> {
    get_thread_task_graph_by_id(app_state, deployment_id, graph_id).await?;
    ListThreadTaskEdgesQuery::new(graph_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn get_thread_task_graph_summary(
    app_state: &AppState,
    deployment_id: i64,
    graph_id: i64,
) -> Result<ThreadTaskGraphSummary, AppError> {
    get_thread_task_graph_by_id(app_state, deployment_id, graph_id).await?;
    GetThreadTaskGraphSummaryQuery::new(graph_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?
        .ok_or_else(|| AppError::NotFound("Thread task graph summary not found".to_string()))
}
