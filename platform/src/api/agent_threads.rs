use crate::application::{
    agent_threads as agent_threads_app,
    response::{ApiResult, PaginatedResponse},
};
use crate::middleware::RequireDeployment;
use axum::extract::{Json, Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use common::state::AppState;
use dto::json::deployment::{
    CreateActorProjectRequest, CreateActorRequest, CreateAgentThreadRequest, ExecuteAgentRequest,
    ExecuteAgentResponse, SearchActorProjectThreadsRequest, SearchActorProjectsRequest,
    UpdateActorProjectRequest, UpdateAgentThreadRequest, CreateProjectTaskBoardItemRequest,
    UpdateProjectTaskBoardItemRequest,
};
use models::{
    Actor, ActorProject, AgentThread, AgentThreadState, ConversationRecord, ProjectTaskBoard,
    ProjectTaskBoardItem, ProjectTaskBoardItemAssignment, ProjectTaskBoardItemEvent,
    ProjectTaskBoardItemRelation, ThreadEvent, ThreadTaskEdge, ThreadTaskGraph,
    ThreadTaskGraphSummary, ThreadTaskNode,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct IncludeArchivedParams {
    pub include_archived: Option<bool>,
}

#[derive(Deserialize)]
pub struct ActorParams {
    pub actor_id: i64,
}

#[derive(Deserialize)]
pub struct ProjectParams {
    pub project_id: i64,
}

#[derive(Deserialize)]
pub struct ThreadParams {
    pub thread_id: i64,
}

#[derive(Deserialize)]
pub struct BoardItemParams {
    pub item_id: i64,
}

#[derive(Deserialize)]
pub struct AppendBoardItemJournalRequest {
    pub summary: String,
    pub details: Option<String>,
    pub body_markdown: Option<String>,
    pub attachments: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct ThreadEventParams {
    pub event_id: i64,
}

#[derive(Deserialize)]
pub struct ThreadTaskGraphParams {
    pub graph_id: i64,
}

#[derive(Deserialize)]
pub struct LimitParams {
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct IncludeTerminalParams {
    pub include_terminal: Option<bool>,
}

#[derive(Deserialize)]
pub struct ActorIdQuery {
    pub actor_id: i64,
}

#[derive(Deserialize)]
pub struct ActorProjectsListQuery {
    pub actor_id: i64,
    pub include_archived: Option<bool>,
}

#[derive(Deserialize)]
pub struct ThreadMessagesQuery {
    pub limit: Option<i64>,
    pub before_id: Option<i64>,
    pub after_id: Option<i64>,
}

#[derive(Deserialize)]
pub struct FilesystemQuery {
    pub path: Option<String>,
}

#[derive(Deserialize)]
pub struct McpActorQuery {
    pub actor_id: i64,
}

#[derive(Deserialize)]
pub struct McpServerParams {
    pub mcp_server_id: i64,
}

#[derive(Serialize)]
pub struct CursorPage<T> {
    pub data: Vec<T>,
    pub limit: i64,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Serialize)]
pub struct ListMessagesResponse {
    pub data: Vec<ConversationRecord>,
    pub has_more: bool,
}

pub async fn list_actors(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<IncludeArchivedParams>,
) -> ApiResult<PaginatedResponse<Actor>> {
    let actors = agent_threads_app::list_actors(
        &app_state,
        deployment_id,
        params.include_archived.unwrap_or(false),
    )
    .await?;
    Ok(PaginatedResponse::from(actors).into())
}

pub async fn create_actor(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateActorRequest>,
) -> ApiResult<Actor> {
    let actor = agent_threads_app::create_actor(&app_state, deployment_id, request).await?;
    Ok(actor.into())
}

pub async fn get_actor_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ActorParams>,
) -> ApiResult<Actor> {
    let actor =
        agent_threads_app::get_actor_by_id(&app_state, deployment_id, params.actor_id).await?;
    Ok(actor.into())
}

pub async fn list_actor_projects(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ActorParams>,
    Query(query): Query<IncludeArchivedParams>,
) -> ApiResult<PaginatedResponse<ActorProject>> {
    let projects = agent_threads_app::list_actor_projects(
        &app_state,
        deployment_id,
        params.actor_id,
        query.include_archived.unwrap_or(false),
    )
    .await?;
    Ok(PaginatedResponse::from(projects).into())
}

pub async fn create_actor_project(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ActorParams>,
    Json(request): Json<CreateActorProjectRequest>,
) -> ApiResult<ActorProject> {
    let project = agent_threads_app::create_actor_project(
        &app_state,
        deployment_id,
        params.actor_id,
        request,
    )
    .await?;
    Ok(project.into())
}

pub async fn list_actor_projects_flat(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<ActorProjectsListQuery>,
) -> ApiResult<PaginatedResponse<ActorProject>> {
    let projects = agent_threads_app::list_actor_projects(
        &app_state,
        deployment_id,
        query.actor_id,
        query.include_archived.unwrap_or(false),
    )
    .await?;
    Ok(PaginatedResponse::from(projects).into())
}

pub async fn search_actor_projects(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<SearchActorProjectsRequest>,
) -> ApiResult<CursorPage<ActorProject>> {
    let page = agent_threads_app::search_actor_projects(
        &app_state,
        deployment_id,
        query.actor_id,
        query.q.unwrap_or_default(),
        query.limit.unwrap_or(20),
        query.cursor,
    )
    .await?;
    Ok(CursorPage {
        data: page.data,
        limit: page.limit,
        has_more: page.has_more,
        next_cursor: page.next_cursor,
    }
    .into())
}

pub async fn create_actor_project_flat(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<ActorIdQuery>,
    Json(request): Json<CreateActorProjectRequest>,
) -> ApiResult<ActorProject> {
    let project =
        agent_threads_app::create_actor_project(&app_state, deployment_id, query.actor_id, request)
            .await?;
    Ok(project.into())
}

pub async fn list_actor_mcp_servers(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<McpActorQuery>,
) -> ApiResult<PaginatedResponse<agent_threads_app::ActorMcpServerSummary>> {
    let servers = agent_threads_app::list_actor_mcp_servers(
        &app_state,
        deployment_id,
        query.actor_id,
    )
    .await?;
    Ok(PaginatedResponse::from(servers).into())
}

pub async fn connect_actor_mcp_server(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<McpServerParams>,
    Query(query): Query<McpActorQuery>,
) -> ApiResult<agent_threads_app::ActorMcpServerConnectResponse> {
    let response = agent_threads_app::build_actor_mcp_server_connect_url(
        &app_state,
        deployment_id,
        query.actor_id,
        params.mcp_server_id,
    )
    .await?;
    Ok(response.into())
}

pub async fn disconnect_actor_mcp_server(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<McpServerParams>,
    Query(query): Query<McpActorQuery>,
) -> ApiResult<serde_json::Value> {
    agent_threads_app::disconnect_actor_mcp_server(
        &app_state,
        deployment_id,
        query.actor_id,
        params.mcp_server_id,
    )
    .await?;
    Ok(serde_json::json!({ "success": true }).into())
}

pub async fn get_actor_project_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ProjectParams>,
) -> ApiResult<ActorProject> {
    let project =
        agent_threads_app::get_actor_project_by_id(&app_state, deployment_id, params.project_id)
            .await?;
    Ok(project.into())
}

pub async fn update_actor_project(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ProjectParams>,
    Json(request): Json<UpdateActorProjectRequest>,
) -> ApiResult<ActorProject> {
    let project = agent_threads_app::update_actor_project(
        &app_state,
        deployment_id,
        params.project_id,
        request,
    )
    .await?;
    Ok(project.into())
}

pub async fn archive_actor_project(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ProjectParams>,
) -> ApiResult<ActorProject> {
    let project = agent_threads_app::set_actor_project_archived(
        &app_state,
        deployment_id,
        params.project_id,
        true,
    )
    .await?;
    Ok(project.into())
}

pub async fn unarchive_actor_project(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ProjectParams>,
) -> ApiResult<ActorProject> {
    let project = agent_threads_app::set_actor_project_archived(
        &app_state,
        deployment_id,
        params.project_id,
        false,
    )
    .await?;
    Ok(project.into())
}

pub async fn list_agent_threads(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ProjectParams>,
    Query(query): Query<IncludeArchivedParams>,
) -> ApiResult<PaginatedResponse<AgentThread>> {
    let threads = agent_threads_app::list_agent_threads(
        &app_state,
        deployment_id,
        params.project_id,
        query.include_archived.unwrap_or(false),
    )
    .await?;
    Ok(PaginatedResponse::from(threads).into())
}

pub async fn create_agent_thread(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ProjectParams>,
    Json(request): Json<CreateAgentThreadRequest>,
) -> ApiResult<AgentThread> {
    let thread = agent_threads_app::create_agent_thread(
        &app_state,
        deployment_id,
        params.project_id,
        request,
    )
    .await?;
    Ok(thread.into())
}

pub async fn search_actor_project_threads(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<SearchActorProjectThreadsRequest>,
) -> ApiResult<CursorPage<AgentThread>> {
    let page = agent_threads_app::search_actor_project_threads(
        &app_state,
        deployment_id,
        query.actor_id,
        query.q.unwrap_or_default(),
        query.limit.unwrap_or(20),
        query.cursor,
    )
    .await?;
    Ok(CursorPage {
        data: page.data,
        limit: page.limit,
        has_more: page.has_more,
        next_cursor: page.next_cursor,
    }
    .into())
}

pub async fn get_agent_thread_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadParams>,
) -> ApiResult<AgentThread> {
    let thread =
        agent_threads_app::get_agent_thread_by_id(&app_state, deployment_id, params.thread_id)
            .await?;
    Ok(thread.into())
}

pub async fn update_agent_thread(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadParams>,
    Json(request): Json<UpdateAgentThreadRequest>,
) -> ApiResult<AgentThread> {
    let thread = agent_threads_app::update_agent_thread(
        &app_state,
        deployment_id,
        params.thread_id,
        request,
    )
    .await?;
    Ok(thread.into())
}

pub async fn archive_agent_thread(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadParams>,
) -> ApiResult<AgentThread> {
    let thread = agent_threads_app::set_agent_thread_archived(
        &app_state,
        deployment_id,
        params.thread_id,
        true,
    )
    .await?;
    Ok(thread.into())
}

pub async fn unarchive_agent_thread(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadParams>,
) -> ApiResult<AgentThread> {
    let thread = agent_threads_app::set_agent_thread_archived(
        &app_state,
        deployment_id,
        params.thread_id,
        false,
    )
    .await?;
    Ok(thread.into())
}

pub async fn execute_agent_thread_async(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadParams>,
    Json(request): Json<ExecuteAgentRequest>,
) -> ApiResult<ExecuteAgentResponse> {
    let response = agent_threads_app::execute_agent_thread_async(
        &app_state,
        deployment_id,
        params.thread_id,
        request,
    )
    .await?;
    Ok(response.into())
}

pub async fn get_project_task_board_by_project_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ProjectParams>,
) -> ApiResult<ProjectTaskBoard> {
    let board = agent_threads_app::get_project_task_board_by_project_id(
        &app_state,
        deployment_id,
        params.project_id,
    )
    .await?;
    Ok(board.into())
}

pub async fn list_project_task_board_items(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ProjectParams>,
) -> ApiResult<PaginatedResponse<ProjectTaskBoardItem>> {
    let items = agent_threads_app::list_project_task_board_items(
        &app_state,
        deployment_id,
        params.project_id,
    )
    .await?;
    Ok(PaginatedResponse::from(items).into())
}

pub async fn create_project_task_board_item(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ProjectParams>,
    Json(request): Json<CreateProjectTaskBoardItemRequest>,
) -> ApiResult<ProjectTaskBoardItem> {
    let item = agent_threads_app::create_project_task_board_item(
        &app_state,
        deployment_id,
        params.project_id,
        request,
    )
    .await?;
    Ok(item.into())
}

pub async fn get_project_task_board_item_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<BoardItemParams>,
) -> ApiResult<ProjectTaskBoardItem> {
    let item = agent_threads_app::get_project_task_board_item_by_id(
        &app_state,
        deployment_id,
        params.item_id,
    )
    .await?;
    Ok(item.into())
}

pub async fn list_project_task_board_item_filesystem(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((project_id, item_id)): Path<(i64, i64)>,
    Query(query): Query<FilesystemQuery>,
) -> ApiResult<agent_threads_app::TaskWorkspaceListing> {
    let listing = agent_threads_app::list_project_task_board_item_filesystem(
        &app_state,
        deployment_id,
        project_id,
        item_id,
        query.path.unwrap_or_default(),
    )
    .await?;
    Ok(listing.into())
}

pub async fn get_project_task_board_item_filesystem_file(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((project_id, item_id)): Path<(i64, i64)>,
    Query(query): Query<FilesystemQuery>,
) -> ApiResult<agent_threads_app::TaskWorkspaceFileContent> {
    let path = query
        .path
        .ok_or_else(|| crate::application::response::ApiErrorResponse::bad_request("path query parameter is required"))?;
    let content = agent_threads_app::get_project_task_board_item_filesystem_file(
        &app_state,
        deployment_id,
        project_id,
        item_id,
        path,
    )
    .await?;
    Ok(content.into())
}

pub async fn list_project_task_board_item_events(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<BoardItemParams>,
) -> ApiResult<PaginatedResponse<ProjectTaskBoardItemEvent>> {
    let events = agent_threads_app::list_project_task_board_item_events(
        &app_state,
        deployment_id,
        params.item_id,
    )
    .await?;
    Ok(PaginatedResponse::from(events).into())
}

pub async fn list_project_task_board_item_assignments(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<BoardItemParams>,
) -> ApiResult<PaginatedResponse<ProjectTaskBoardItemAssignment>> {
    let assignments = agent_threads_app::list_project_task_board_item_assignments(
        &app_state,
        deployment_id,
        params.item_id,
    )
    .await?;
    Ok(PaginatedResponse::from(assignments).into())
}

pub async fn list_project_task_board_item_relations(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<BoardItemParams>,
) -> ApiResult<PaginatedResponse<ProjectTaskBoardItemRelation>> {
    let relations = agent_threads_app::list_project_task_board_item_relations(
        &app_state,
        deployment_id,
        params.item_id,
    )
    .await?;
    Ok(PaginatedResponse::from(relations).into())
}

pub async fn append_project_task_board_item_journal(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<BoardItemParams>,
    Json(request): Json<AppendBoardItemJournalRequest>,
) -> ApiResult<ProjectTaskBoardItemEvent> {
    let event = agent_threads_app::append_project_task_board_item_journal_entry(
        &app_state,
        deployment_id,
        params.item_id,
        request.summary,
        request.details,
        request.body_markdown,
        request.attachments,
    )
    .await?;
    Ok(event.into())
}

pub async fn update_project_task_board_item(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((project_id, item_id)): Path<(i64, i64)>,
    Json(request): Json<UpdateProjectTaskBoardItemRequest>,
) -> ApiResult<ProjectTaskBoardItem> {
    let item = agent_threads_app::update_project_task_board_item(
        &app_state,
        deployment_id,
        project_id,
        item_id,
        request,
    )
    .await?;
    Ok(item.into())
}

pub async fn archive_project_task_board_item(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((project_id, item_id)): Path<(i64, i64)>,
) -> ApiResult<ProjectTaskBoardItem> {
    let item = agent_threads_app::set_project_task_board_item_archived(
        &app_state,
        deployment_id,
        project_id,
        item_id,
        true,
    )
    .await?;
    Ok(item.into())
}

pub async fn unarchive_project_task_board_item(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((project_id, item_id)): Path<(i64, i64)>,
) -> ApiResult<ProjectTaskBoardItem> {
    let item = agent_threads_app::set_project_task_board_item_archived(
        &app_state,
        deployment_id,
        project_id,
        item_id,
        false,
    )
    .await?;
    Ok(item.into())
}

pub async fn get_agent_thread_state(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadParams>,
) -> ApiResult<AgentThreadState> {
    let thread =
        agent_threads_app::get_agent_thread_state(&app_state, deployment_id, params.thread_id)
            .await?;
    Ok(thread.into())
}

pub async fn list_pending_thread_events(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadParams>,
) -> ApiResult<PaginatedResponse<ThreadEvent>> {
    let events =
        agent_threads_app::list_pending_thread_events(&app_state, deployment_id, params.thread_id)
            .await?;
    Ok(PaginatedResponse::from(events).into())
}

pub async fn get_thread_event_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadEventParams>,
) -> ApiResult<ThreadEvent> {
    let event =
        agent_threads_app::get_thread_event_by_id(&app_state, deployment_id, params.event_id)
            .await?;
    Ok(event.into())
}

pub async fn list_assignments_for_thread(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadParams>,
) -> ApiResult<PaginatedResponse<ProjectTaskBoardItemAssignment>> {
    let assignments =
        agent_threads_app::list_assignments_for_thread(&app_state, deployment_id, params.thread_id)
            .await?;
    Ok(PaginatedResponse::from(assignments).into())
}

pub async fn get_latest_thread_task_graph(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadParams>,
) -> ApiResult<Option<ThreadTaskGraph>> {
    let graph = agent_threads_app::get_latest_thread_task_graph(
        &app_state,
        deployment_id,
        params.thread_id,
    )
    .await?;
    Ok(graph.into())
}

pub async fn get_thread_task_graph_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadTaskGraphParams>,
) -> ApiResult<ThreadTaskGraph> {
    let graph =
        agent_threads_app::get_thread_task_graph_by_id(&app_state, deployment_id, params.graph_id)
            .await?;
    Ok(graph.into())
}

pub async fn list_thread_task_nodes(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadTaskGraphParams>,
    Query(query): Query<IncludeTerminalParams>,
) -> ApiResult<PaginatedResponse<ThreadTaskNode>> {
    let nodes = agent_threads_app::list_thread_task_nodes(
        &app_state,
        deployment_id,
        params.graph_id,
        query.include_terminal.unwrap_or(true),
    )
    .await?;
    Ok(PaginatedResponse::from(nodes).into())
}

pub async fn list_thread_task_edges(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadTaskGraphParams>,
) -> ApiResult<PaginatedResponse<ThreadTaskEdge>> {
    let edges =
        agent_threads_app::list_thread_task_edges(&app_state, deployment_id, params.graph_id)
            .await?;
    Ok(PaginatedResponse::from(edges).into())
}

pub async fn get_thread_task_graph_summary(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadTaskGraphParams>,
) -> ApiResult<ThreadTaskGraphSummary> {
    let summary = agent_threads_app::get_thread_task_graph_summary(
        &app_state,
        deployment_id,
        params.graph_id,
    )
    .await?;
    Ok(summary.into())
}

pub async fn get_agent_thread_messages(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadParams>,
    Query(query): Query<ThreadMessagesQuery>,
) -> ApiResult<ListMessagesResponse> {
    let (messages, has_more) = agent_threads_app::list_thread_messages(
        &app_state,
        deployment_id,
        params.thread_id,
        query.limit.unwrap_or(50),
        query.before_id,
        query.after_id,
    )
    .await?;
    Ok(ListMessagesResponse {
        data: messages,
        has_more,
    }
    .into())
}

pub async fn list_agent_thread_filesystem(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadParams>,
    Query(query): Query<FilesystemQuery>,
) -> ApiResult<agent_threads_app::TaskWorkspaceListing> {
    let listing = agent_threads_app::list_thread_filesystem(
        &app_state,
        deployment_id,
        params.thread_id,
        query.path.unwrap_or_default(),
    )
    .await?;
    Ok(listing.into())
}

pub async fn get_agent_thread_filesystem_file(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadParams>,
    Query(query): Query<FilesystemQuery>,
) -> Result<impl IntoResponse, crate::application::response::ApiErrorResponse> {
    let path = query
        .path
        .ok_or_else(|| crate::application::response::ApiErrorResponse::bad_request("path query parameter is required"))?;
    let (body, mime_type, cleaned_path) = agent_threads_app::get_thread_filesystem_file(
        &app_state,
        deployment_id,
        params.thread_id,
        path,
    )
    .await
    .map_err(|err| match err {
        common::error::AppError::NotFound(message) => {
            crate::application::response::ApiErrorResponse::new(StatusCode::NOT_FOUND, message)
        }
        common::error::AppError::S3(message)
        | common::error::AppError::Internal(message) => {
            crate::application::response::ApiErrorResponse::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                message,
            )
        }
        common::error::AppError::Database(message) => crate::application::response::ApiErrorResponse::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            message.to_string(),
        ),
        _ => crate::application::response::ApiErrorResponse::new(
            StatusCode::BAD_REQUEST,
            err.to_string(),
        ),
    })?;
    let filename = agent_threads_app::sanitize_download_filename(&cleaned_path);
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&mime_type).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", filename))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    Ok((headers, body))
}
