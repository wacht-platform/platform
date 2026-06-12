use commands::{
    CreateActorCommand, CreateActorProjectCommand, CreateAgentThreadCommand,
    CreateProjectTaskBoardItemCommand, CreateProjectTaskScheduleCommand,
    DeleteProjectTaskScheduleByTaskKeyCommand, EnsureProjectTaskBoardCommand,
    SetActorProjectDefaultThreadsCommand, UpdateProjectTaskBoardItemMountsCommand,
    UpdateProjectTaskScheduleCommand, UpsertThreadAgentAssignmentCommand,
};
use common::ReadConsistency;
use common::ResultExt;
use common::error::AppError;
use dto::json::deployment::{
    CreateActorProjectRequest, CreateActorRequest, CreateAgentThreadRequest,
    CreateProjectTaskBoardItemRequest, ExecuteAgentRequest, ExecuteAgentResponse,
    UpdateActorProjectRequest, UpdateAgentThreadRequest, UpdateProjectTaskBoardItemRequest,
};
use models::{
    Actor, ActorProject, AgentThread, AgentThreadState, ConversationRecord, ProjectTaskBoard,
    ProjectTaskBoardItem, ProjectTaskBoardItemAssignment, ProjectTaskBoardItemRelation,
    ScheduleTemplatePayload, ThreadTaskEdge, ThreadTaskGraph, ThreadTaskGraphSummary,
    ThreadTaskNode,
};
use queries::{
    GetActorByIdQuery, GetActorProjectByIdQuery, GetAgentThreadByIdQuery, GetAgentThreadStateQuery,
    GetAiAgentByIdQuery, GetLatestThreadTaskGraphQuery, GetMcpServerByIdQuery,
    GetProjectTaskBoardByIdQuery, GetProjectTaskBoardByProjectIdQuery,
    GetProjectTaskBoardItemByIdQuery, GetProjectTaskScheduleByTaskKeyQuery,
    GetThreadTaskGraphByIdQuery, GetThreadTaskGraphSummaryQuery, ListActorProjectsQuery,
    ListActorsQuery, ListAgentThreadsQuery, ListAssignmentsForThreadQuery,
    ListProjectTaskBoardItemAssignmentsQuery, ListProjectTaskBoardItemRelationsQuery,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<TaskWorkspaceMount>,
}

#[derive(Clone, Serialize)]
pub struct TaskWorkspaceMount {
    pub mount_path: String,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(
        "{}|{}",
        ts.timestamp_nanos_opt().unwrap_or(0),
        id
    ))
}

fn decode_time_id_cursor(
    cursor: &str,
) -> Result<Option<(chrono::DateTime<chrono::Utc>, i64)>, AppError> {
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

pub const MAX_TASK_WORKSPACE_UPLOAD_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct WorkspaceUploadInput {
    pub original_name: String,
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct UploadedTaskWorkspaceFile {
    pub path: String,
    pub name: String,
    pub original_name: String,
    pub mime_type: String,
    pub size_bytes: u64,
}

pub async fn upload_task_workspace_files(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
    task_key: &str,
    files: Vec<WorkspaceUploadInput>,
) -> Result<Vec<UploadedTaskWorkspaceFile>, AppError> {
    if files.is_empty() {
        return Ok(Vec::new());
    }

    let deps = common::deps::from_app(app_state).db().enc();
    let storage = commands::ResolveDeploymentStorageCommand::new(deployment_id)
        .execute_with_deps(&deps)
        .await?;

    let base_prefix = task_workspace_storage_prefix(deployment_id, project_id, task_key);
    let mut results = Vec::with_capacity(files.len());

    for file in files {
        let size = file.bytes.len() as u64;
        if size == 0 {
            continue;
        }
        if size > MAX_TASK_WORKSPACE_UPLOAD_BYTES {
            return Err(AppError::BadRequest(format!(
                "file '{}' exceeds {}MB limit",
                file.original_name,
                MAX_TASK_WORKSPACE_UPLOAD_BYTES / (1024 * 1024)
            )));
        }
        let safe_name = common::sanitize_filename(&file.original_name).ok_or_else(|| {
            AppError::BadRequest(format!("filename '{}' is invalid", file.original_name))
        })?;
        let id = app_state.sf.next_id()? as i64;
        let relative_path = format!("uploads/{}_{}", id, safe_name);
        let mime_type = file
            .content_type
            .filter(|ct| !ct.is_empty() && ct != "application/octet-stream")
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let key = storage.object_key(&format!("{}{}", base_prefix, relative_path));
        let request = storage
            .client()
            .put_object()
            .bucket(storage.bucket())
            .key(&key)
            .body(aws_sdk_s3::primitives::ByteStream::from(file.bytes))
            .content_type(mime_type.clone());
        request
            .send()
            .await
            .map_err(|e| AppError::S3(e.to_string()))?;

        results.push(UploadedTaskWorkspaceFile {
            path: format!("/task/{}", relative_path),
            name: Path::new(&relative_path)
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or(&relative_path)
                .to_string(),
            original_name: file.original_name,
            mime_type,
            size_bytes: size,
        });
    }

    Ok(results)
}

pub fn merge_attachments_into_metadata(
    existing: &serde_json::Value,
    new_attachments: &[UploadedTaskWorkspaceFile],
) -> Result<serde_json::Value, AppError> {
    if new_attachments.is_empty() {
        return Ok(existing.clone());
    }
    let mut metadata = match existing {
        serde_json::Value::Object(_) => existing.clone(),
        serde_json::Value::Null => serde_json::Value::Object(Default::default()),
        _ => {
            return Err(AppError::Internal(
                "metadata is not an object; cannot merge attachments".to_string(),
            ));
        }
    };

    let Some(obj) = metadata.as_object_mut() else {
        return Err(AppError::Internal(
            "metadata is not an object; cannot merge attachments".to_string(),
        ));
    };
    let existing_attachments = obj
        .get("attachments")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut merged = existing_attachments;
    for attachment in new_attachments {
        merged.push(serde_json::to_value(attachment).map_err_internal("serialize attachment")?);
    }
    obj.insert("attachments".to_string(), serde_json::Value::Array(merged));
    Ok(metadata)
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
        return Err(AppError::BadRequest(
            "path query parameter is required".to_string(),
        ));
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
            .map(|ext| {
                matches!(
                    ext,
                    "txt"
                        | "md"
                        | "json"
                        | "js"
                        | "ts"
                        | "tsx"
                        | "jsx"
                        | "py"
                        | "rs"
                        | "go"
                        | "yml"
                        | "yaml"
                        | "xml"
                        | "html"
                        | "css"
                        | "sql"
                        | "toml"
                )
            })
            .unwrap_or(false)
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
            let relative = key
                .strip_prefix(&base_key_prefix)
                .unwrap_or(key)
                .to_string();
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
                modified_at: object.last_modified().map(|v| {
                    chrono::DateTime::from_timestamp(v.secs(), 0).unwrap_or_else(Utc::now)
                }),
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
        mounts: Vec::new(),
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

    let rows = queries::SearchActorProjectsQuery {
        deployment_id,
        actor_id,
        like,
        cursor_updated_at: cursor.map(|v| v.0),
        cursor_id: cursor.map(|v| v.1),
        limit: limit + 1,
    }
    .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
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
    let selected_agent_id = request.agent_id.map(i64::from);

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

    let mut tx = app_state.db_router.writer().begin().await?;

    let project = command.execute_with_db(&mut *tx).await?;

    let thread_instructions = templatekit::render_project_instructions(
        &project.name,
        project.description.as_deref(),
        None,
    )?;

    let coordinator_thread_id = app_state.sf.next_id()? as i64;
    CreateAgentThreadCommand::new(
        coordinator_thread_id,
        deployment_id,
        actor_id,
        project.id,
        "Coordinator".to_string(),
        models::agent_thread::purpose::COORDINATOR.to_string(),
        "idle".to_string(),
    )
    .with_thread_purpose(models::agent_thread::purpose::COORDINATOR.to_string())
    .with_responsibility("Project coordinator".to_string())
    .mark_reusable()
    .with_system_instructions(thread_instructions.clone())
    .execute_with_db(&mut *tx)
    .await?;

    let review_thread_id = app_state.sf.next_id()? as i64;
    CreateAgentThreadCommand::new(
        review_thread_id,
        deployment_id,
        actor_id,
        project.id,
        "Review".to_string(),
        models::agent_thread::purpose::REVIEW.to_string(),
        "idle".to_string(),
    )
    .with_thread_purpose(models::agent_thread::purpose::REVIEW.to_string())
    .with_responsibility("Project reviewer".to_string())
    .mark_reusable()
    .allow_assignments()
    .with_system_instructions(thread_instructions)
    .execute_with_db(&mut *tx)
    .await?;

    SetActorProjectDefaultThreadsCommand::new(
        project.id,
        deployment_id,
        coordinator_thread_id,
        review_thread_id,
    )
    .execute_with_db(&mut *tx)
    .await?;

    if let Some(agent_id) = selected_agent_id {
        // The coordinator is always the selected agent; the reviewer defaults to
        // self unless the agent designates one of its sub-agents as reviewer.
        let reviewer_agent_id = GetAiAgentByIdQuery::new(deployment_id, agent_id)
            .execute_with_db(&mut *tx)
            .await?
            .reviewer_agent_id
            .unwrap_or(agent_id);
        UpsertThreadAgentAssignmentCommand::new(coordinator_thread_id, agent_id)
            .execute_with_db(&mut *tx)
            .await?;
        UpsertThreadAgentAssignmentCommand::new(review_thread_id, reviewer_agent_id)
            .execute_with_db(&mut *tx)
            .await?;
    }

    // Every project gets its task board at creation time, so createProjectTaskBoardItem
    // and delegateProjectTask never have to bootstrap one.
    EnsureProjectTaskBoardCommand::new(
        app_state.sf.next_id()? as i64,
        deployment_id,
        actor_id,
        project.id,
        format!("Project {} Task Board", project.id),
        "active".to_string(),
    )
    .execute_with_db(&mut *tx)
    .await?;

    tx.commit().await?;

    get_actor_project_by_id(app_state, deployment_id, project.id).await
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
    let agent_id = agent_id.map(i64::from);

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
    let is_conversation =
        resolved_thread_purpose.as_str() == models::agent_thread::purpose::CONVERSATION;
    let resolved_responsibility = responsibility.filter(|value| !value.trim().is_empty());
    let resolved_capability_tags = capability_tags.unwrap_or_default();
    let resolved_reusable = reusable.unwrap_or(false);
    let resolved_accepts_assignments = accepts_assignments.unwrap_or(false);
    let generated_system_instructions = templatekit::render_project_instructions(
        &project.name,
        project.description.as_deref(),
        None,
    )?;

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
        // A user-facing conversation defaults to the agent itself unless the agent
        // designates one of its sub-agents as its conversation agent.
        let bound_agent_id = if is_conversation {
            GetAiAgentByIdQuery::new(deployment_id, agent_id)
                .execute_with_db(app_state.db_router.writer())
                .await?
                .conversation_agent_id
                .unwrap_or(agent_id)
        } else {
            agent_id
        };
        UpsertThreadAgentAssignmentCommand::new(thread.id, bound_agent_id)
            .execute_with_db(app_state.db_router.writer())
            .await?;
    }

    get_agent_thread_by_id(app_state, deployment_id, thread_id).await
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

    let rows = queries::SearchAgentThreadsByActorQuery {
        deployment_id,
        actor_id,
        like,
        cursor_last_activity_at: cursor.map(|v| v.0),
        cursor_id: cursor.map(|v| v.1),
        limit: limit + 1,
    }
    .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
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
    let name = request
        .name
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let description = request.description.map(|v| v.trim().to_string());
    let status = request
        .status
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    if name.is_none() && description.is_none() && status.is_none() {
        return Ok(existing);
    }

    commands::UpdateActorProjectCommand {
        project_id,
        deployment_id,
        name,
        description,
        status,
    }
    .execute_with_db(app_state.db_router.writer())
    .await
}

pub async fn set_actor_project_archived(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
    archived: bool,
) -> Result<ActorProject, AppError> {
    get_actor_project_by_id(app_state, deployment_id, project_id).await?;
    commands::SetActorProjectArchivedCommand {
        project_id,
        deployment_id,
        archived,
    }
    .execute_with_db(app_state.db_router.writer())
    .await
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
    let agent_id = agent_id.map(i64::from);

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
    let thread = command
        .execute_with_db(app_state.db_router.writer())
        .await?;

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

    let updated = commands::SetAgentThreadArchivedCommand {
        thread_id,
        deployment_id,
        archived,
    }
    .execute_with_db(app_state.db_router.writer())
    .await?;

    if archived {
        commands::DeleteSubscriptionsForThreadCommand { thread_id }
            .execute(app_state.db_router.writer())
            .await?;
    }

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
    let mut data = queries::ListThreadMessagesForUserQuery::new(thread_id, limit + 1)
        .with_before_id(before_id)
        .with_after_id(after_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    let has_more = data.len() as i64 > limit;
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
            mounts: Vec::new(),
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
        return Err(AppError::BadRequest(
            "requested path is a directory".to_string(),
        ));
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
    let (body, mime_type) =
        read_workspace_file(app_state, deployment_id, base_prefix, relative).await?;
    Ok((body, mime_type, cleaned))
}

pub async fn list_actor_mcp_servers(
    app_state: &AppState,
    deployment_id: i64,
    actor_id: i64,
) -> Result<Vec<ActorMcpServerSummary>, AppError> {
    get_actor_by_id(app_state, deployment_id, actor_id).await?;
    let entries = queries::GetActorMcpConnectionsQuery::new(deployment_id, actor_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    let now = Utc::now();
    let result = entries
        .into_iter()
        .map(|entry| {
            let server = entry.server;
            let connection_metadata = entry.connection_metadata;
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
                if let Some(metadata) = connection_metadata.as_ref() {
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
    let auth = server.config.auth.ok_or_else(|| {
        AppError::BadRequest("This MCP server does not require actor consent".to_string())
    })?;

    let (client_id, auth_url, token_url, scopes, resource) = match auth {
        models::McpAuthConfig::OAuthAuthorizationCodePublicPkce {
            client_id,
            auth_url,
            token_url,
            scopes,
            resource,
            ..
        } => (
            client_id.ok_or_else(|| {
                AppError::BadRequest("MCP server client_id is missing".to_string())
            })?,
            auth_url.ok_or_else(|| {
                AppError::BadRequest("MCP server auth_url is missing".to_string())
            })?,
            token_url.ok_or_else(|| {
                AppError::BadRequest("MCP server token_url is missing".to_string())
            })?,
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
            auth_url.ok_or_else(|| {
                AppError::BadRequest("MCP server auth_url is missing".to_string())
            })?,
            token_url.ok_or_else(|| {
                AppError::BadRequest("MCP server token_url is missing".to_string())
            })?,
            scopes,
            resource,
        ),
        _ => {
            return Err(AppError::BadRequest(
                "This MCP server does not require actor consent".to_string(),
            ));
        }
    };

    let state = generate_random_base64_url(24)?;
    let code_verifier = generate_random_base64_url(32)?;
    let redirect_uri = "https://agentlink.wacht.services/service/mcp/consent/callback".to_string();
    commands::CreateMcpOAuthStateCommand {
        state: state.clone(),
        deployment_id,
        actor_id,
        mcp_server_id,
        code_verifier: code_verifier.clone(),
        client_id: client_id.clone(),
        token_url: token_url.clone(),
        redirect_uri: redirect_uri.clone(),
        resource: resource.clone(),
        expires_at: Utc::now() + chrono::Duration::minutes(15),
    }
    .execute_with_db(app_state.db_router.writer())
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
    commands::DeleteActorMcpConnectionCommand {
        deployment_id,
        actor_id,
        mcp_server_id,
    }
    .execute_with_db(app_state.db_router.writer())
    .await
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
    attachments: Vec<WorkspaceUploadInput>,
) -> Result<ProjectTaskBoardItem, AppError> {
    let (project, board) = tokio::try_join!(
        get_actor_project_by_id(app_state, deployment_id, project_id),
        get_project_task_board_by_project_id(app_state, deployment_id, project_id),
    )?;
    let item_id = app_state.sf.next_id()? as i64;
    let task_key = format!("TASK-{}", item_id);
    let status = request.status.unwrap_or_else(|| "pending".to_string());
    let assigned_thread_id = project.coordinator_thread_id;
    let schedule = parse_schedule_request(
        request.schedule_kind.as_deref(),
        request.next_run_at,
        request.interval_seconds,
    )?;
    let requested_mounts = request.mounts.clone();
    let mounts_value = validate_and_serialize_mounts(request.mounts.as_deref())?
        .unwrap_or_else(|| serde_json::json!([]));

    let uploaded =
        upload_task_workspace_files(app_state, deployment_id, project_id, &task_key, attachments)
            .await?;
    let metadata = merge_attachments_into_metadata(&serde_json::json!({}), &uploaded)?;

    let mut tx = app_state.db_router.writer().begin().await?;

    let mut item = CreateProjectTaskBoardItemCommand {
        id: item_id,
        board_id: board.id,
        task_key,
        title: request.title.trim().to_string(),
        description: request.description,
        status,
        assigned_thread_id,
        metadata,
        mounts: mounts_value,
        exclusive_owner_agent_id: None,
    }
    .execute_with_db(&mut *tx)
    .await?;

    if let Some(schedule) = schedule {
        let schedule = CreateProjectTaskScheduleCommand {
            id: app_state.sf.next_id()? as i64,
            board_id: board.id,
            project_id,
            task_key: item.task_key.clone(),
            template_payload: build_schedule_template_payload(&item),
            schedule_kind: schedule.kind,
            interval_seconds: schedule.interval_seconds,
            next_run_at: schedule.next_run_at,
            overlap_policy: None,
            mounts: requested_mounts,
        }
        .execute_with_db(&mut *tx)
        .await?;
        item = commands::AttachProjectTaskBoardItemScheduleCommand {
            board_id: board.id,
            task_key: item.task_key.clone(),
            schedule_id: schedule.id,
            mounts: schedule.mounts,
        }
        .execute_with_db(&mut *tx)
        .await?;
    }

    let routed = if let Some(coordinator_thread_id) = project.coordinator_thread_id {
        commands::InsertTaskRoutingEvent {
            event_log_id: app_state.sf.next_id()? as i64,
            deployment_id,
            coordinator_thread_id,
            board_item: &item,
            idempotency_key: format!("task_routing_{}_{}", item.id, item.state_version),
            summary: commands::build_task_routing_summary(&item, 0),
            note: None,
            caused_by_event_id: None,
            routing_reason: models::thread_event::routing_reason::TASK_CREATED,
            previous_status: None,
            changed_fields: Vec::new(),
            last_assignment_result_status: None,
        }
        .execute(&mut *tx)
        .await?;
        true
    } else {
        false
    };

    tx.commit().await?;

    if routed {
        commands::event_log::nudge_dispatcher(&app_state.nats_client).await;
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

pub async fn update_project_task_board_item(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
    item_id: i64,
    request: UpdateProjectTaskBoardItemRequest,
    attachments: Vec<WorkspaceUploadInput>,
) -> Result<ProjectTaskBoardItem, AppError> {
    let project_query = GetActorProjectByIdQuery::new(project_id, deployment_id);
    let (item, board, project_opt) = tokio::try_join!(
        get_project_task_board_item_by_id(app_state, deployment_id, item_id),
        get_project_task_board_by_project_id(app_state, deployment_id, project_id),
        project_query.execute_with_db(app_state.db_router.reader(ReadConsistency::Strong)),
    )?;
    if item.board_id != board.id {
        return Err(AppError::NotFound(
            "Project task board item not found".to_string(),
        ));
    }
    let project = project_opt.ok_or_else(|| AppError::NotFound("Project not found".to_string()))?;

    let uploaded = upload_task_workspace_files(
        app_state,
        deployment_id,
        project_id,
        &item.task_key,
        attachments,
    )
    .await?;

    let clear_schedule = request.clear_schedule.unwrap_or(false);
    let schedule = parse_schedule_request(
        request.schedule_kind.as_deref(),
        request.next_run_at,
        request.interval_seconds,
    )?;
    if clear_schedule && schedule.is_some() {
        return Err(AppError::BadRequest(
            "clear_schedule cannot be combined with new schedule fields".to_string(),
        ));
    }
    let mounts_value = validate_and_serialize_mounts(request.mounts.as_deref())?;
    let requested_mounts = request.mounts.clone();

    let edit_outcome = commands::ApplyBoardItemEditCommand {
        deployment_id,
        board_item_id: item.id,
        coordinator_thread_id: project.coordinator_thread_id,
        title: request.title.clone(),
        description: request.description.clone(),
        status: request.status.clone(),
        preempt_summary: "Preempted by task update.",
        fanout_subscriptions: false,
    }
    .execute(&common::deps::from_app(app_state).db().nats().id())
    .await?;
    let mut current = edit_outcome.item;

    if let Some(mounts) = mounts_value {
        UpdateProjectTaskBoardItemMountsCommand {
            board_id: board.id,
            task_key: current.task_key.clone(),
            mounts: mounts.clone(),
        }
        .execute_with_db(app_state.db_router.writer())
        .await?;
        current.mounts = mounts;
    }

    if clear_schedule {
        DeleteProjectTaskScheduleByTaskKeyCommand::new(board.id, current.task_key.clone())
            .execute_with_db(app_state.db_router.writer())
            .await?;
    } else if let Some(schedule) = schedule {
        let existing =
            GetProjectTaskScheduleByTaskKeyQuery::new(board.id, current.task_key.clone())
                .execute_with_db(app_state.db_router.writer())
                .await?;
        if let Some(existing) = existing {
            let mut command = UpdateProjectTaskScheduleCommand::new(existing.id)
                .with_status(models::project_task_schedule::status::ACTIVE.to_string())
                .with_interval_seconds(schedule.interval_seconds)
                .with_next_run_at(schedule.next_run_at)
                .with_template_payload(build_schedule_template_payload(&current));
            if let Some(mounts) = requested_mounts.clone() {
                command = command.with_mounts(mounts);
            }
            let schedule = command
                .execute_with_db(app_state.db_router.writer())
                .await?;
            current = commands::AttachProjectTaskBoardItemScheduleCommand {
                board_id: board.id,
                task_key: current.task_key.clone(),
                schedule_id: schedule.id,
                mounts: schedule.mounts,
            }
            .execute_with_db(app_state.db_router.writer())
            .await?;
        } else {
            let schedule = CreateProjectTaskScheduleCommand {
                id: app_state.sf.next_id()? as i64,
                board_id: board.id,
                project_id,
                task_key: current.task_key.clone(),
                template_payload: build_schedule_template_payload(&current),
                schedule_kind: schedule.kind,
                interval_seconds: schedule.interval_seconds,
                next_run_at: schedule.next_run_at,
                overlap_policy: None,
                mounts: requested_mounts,
            }
            .execute_with_db(app_state.db_router.writer())
            .await?;
            current = commands::AttachProjectTaskBoardItemScheduleCommand {
                board_id: board.id,
                task_key: current.task_key.clone(),
                schedule_id: schedule.id,
                mounts: schedule.mounts,
            }
            .execute_with_db(app_state.db_router.writer())
            .await?;
        }
    }

    if !uploaded.is_empty() {
        let merged_metadata = merge_attachments_into_metadata(&current.metadata, &uploaded)?;
        commands::ReplaceBoardItemMetadataCommand::new(current.id, merged_metadata.clone())
            .execute_with_db(app_state.db_router.writer())
            .await?;
        current.metadata = merged_metadata;
    }

    if edit_outcome.routed || edit_outcome.preempted || edit_outcome.subscribers_notified > 0 {
        commands::event_log::nudge_dispatcher(&app_state.nats_client).await;
    }

    Ok(current)
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
        return Err(AppError::NotFound(
            "Project task board item not found".to_string(),
        ));
    }

    let updated = commands::SetProjectTaskBoardItemArchivedCommand {
        board_id: board.id,
        item_id,
        archived,
    }
    .execute_with_db(app_state.db_router.writer())
    .await?;

    if archived {
        commands::DeleteSubscriptionsForBoardItemCommand {
            board_item_id: item_id,
        }
        .execute(app_state.db_router.writer())
        .await?;
    }

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
        return Err(AppError::NotFound(
            "Project task board item not found".to_string(),
        ));
    }
    let mut listing = list_workspace_directory(
        app_state,
        deployment_id,
        task_workspace_storage_prefix(deployment_id, project_id, &item.task_key),
        path,
    )
    .await?;
    listing.mounts = models::project_task_schedule::parse_mounts(&item.mounts)
        .unwrap_or_default()
        .into_iter()
        .map(|m| TaskWorkspaceMount {
            mount_path: m.mount_path,
            mode: m.mode,
            description: m.description,
        })
        .collect();
    Ok(listing)
}

pub async fn get_project_task_board_item_filesystem_file_bytes(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
    item_id: i64,
    path: String,
) -> Result<(Vec<u8>, String, String), AppError> {
    let item = get_project_task_board_item_by_id(app_state, deployment_id, item_id).await?;
    let board = get_project_task_board_by_project_id(app_state, deployment_id, project_id).await?;
    if item.board_id != board.id {
        return Err(AppError::NotFound(
            "Project task board item not found".to_string(),
        ));
    }
    let cleaned = sanitize_relative_path(&path)?;
    let (body, mime_type) = read_workspace_file(
        app_state,
        deployment_id,
        task_workspace_storage_prefix(deployment_id, project_id, &item.task_key),
        path,
    )
    .await?;
    Ok((body, mime_type, cleaned))
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
        return Err(AppError::NotFound(
            "Project task board item not found".to_string(),
        ));
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

struct ParsedSchedule {
    kind: String,
    next_run_at: chrono::DateTime<chrono::Utc>,
    interval_seconds: Option<i64>,
}

fn parse_schedule_request(
    schedule_kind: Option<&str>,
    next_run_at: Option<chrono::DateTime<chrono::Utc>>,
    interval_seconds: Option<i64>,
) -> Result<Option<ParsedSchedule>, AppError> {
    if schedule_kind.is_none() && next_run_at.is_none() && interval_seconds.is_none() {
        return Ok(None);
    }
    let kind = schedule_kind
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AppError::BadRequest(
                "Pick a schedule type and choose when the task should run.".to_string(),
            )
        })?;
    let next_run_at = next_run_at.ok_or_else(|| {
        AppError::BadRequest(
            "Pick a schedule type and choose when the task should run.".to_string(),
        )
    })?;
    match kind.as_str() {
        models::project_task_schedule::schedule_kind::ONCE => {
            if interval_seconds.is_some() {
                return Err(AppError::BadRequest(
                    "A one-off task can't have a repeat interval.".to_string(),
                ));
            }
            Ok(Some(ParsedSchedule {
                kind,
                next_run_at,
                interval_seconds: None,
            }))
        }
        models::project_task_schedule::schedule_kind::INTERVAL => {
            let secs = interval_seconds.unwrap_or(0);
            if secs <= 0 {
                return Err(AppError::BadRequest(
                    "A recurring task needs a repeat interval.".to_string(),
                ));
            }
            if secs < commands::MIN_INTERVAL_SECONDS {
                return Err(AppError::BadRequest(
                    "Recurring tasks must repeat at least every 10 minutes.".to_string(),
                ));
            }
            Ok(Some(ParsedSchedule {
                kind,
                next_run_at,
                interval_seconds: Some(secs),
            }))
        }
        _ => Err(AppError::BadRequest(
            "Pick either a one-off run or a recurring interval for this task.".to_string(),
        )),
    }
}

fn build_schedule_template_payload(item: &ProjectTaskBoardItem) -> ScheduleTemplatePayload {
    ScheduleTemplatePayload {
        title: item.title.clone(),
        description: item.description.clone(),
        metadata: item.typed_metadata(),
    }
}

fn validate_and_serialize_mounts(
    mounts: Option<&[models::project_task_schedule::ScheduleMount]>,
) -> Result<Option<serde_json::Value>, AppError> {
    let Some(mounts) = mounts else {
        return Ok(None);
    };
    for m in mounts {
        models::project_task_schedule::validate_mount(m)
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
    }
    let value = serde_json::to_value(mounts).map_err_internal("Failed to serialize mounts")?;
    Ok(Some(value))
}
