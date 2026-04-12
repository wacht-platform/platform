use crate::application::{agent_threads as agent_threads_app, response::ApiResult};
use crate::middleware::RequireDeployment;
use axum::extract::{Json, Path, Query, State};
use common::state::AppState;
use dto::json::deployment::{
    CreateActorProjectRequest, CreateActorRequest, CreateAgentThreadRequest, ExecuteAgentRequest,
    ExecuteAgentResponse,
};
use models::{
    Actor, ActorProject, AgentThread, AgentThreadState, ProjectTaskBoard, ProjectTaskBoardItem,
    ProjectTaskBoardItemAssignment, ProjectTaskBoardItemEvent, ProjectTaskBoardItemRelation,
    ThreadEvent, ThreadTaskEdge, ThreadTaskGraph, ThreadTaskGraphSummary, ThreadTaskNode,
};
use serde::Deserialize;

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

pub async fn list_actors(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<IncludeArchivedParams>,
) -> ApiResult<Vec<Actor>> {
    let actors = agent_threads_app::list_actors(
        &app_state,
        deployment_id,
        params.include_archived.unwrap_or(false),
    )
    .await?;
    Ok(actors.into())
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
) -> ApiResult<Vec<ActorProject>> {
    let projects = agent_threads_app::list_actor_projects(
        &app_state,
        deployment_id,
        params.actor_id,
        query.include_archived.unwrap_or(false),
    )
    .await?;
    Ok(projects.into())
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

pub async fn list_agent_threads(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ProjectParams>,
    Query(query): Query<IncludeArchivedParams>,
) -> ApiResult<Vec<AgentThread>> {
    let threads = agent_threads_app::list_agent_threads(
        &app_state,
        deployment_id,
        params.project_id,
        query.include_archived.unwrap_or(false),
    )
    .await?;
    Ok(threads.into())
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
) -> ApiResult<Vec<ProjectTaskBoardItem>> {
    let items = agent_threads_app::list_project_task_board_items(
        &app_state,
        deployment_id,
        params.project_id,
    )
    .await?;
    Ok(items.into())
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

pub async fn list_project_task_board_item_events(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<BoardItemParams>,
) -> ApiResult<Vec<ProjectTaskBoardItemEvent>> {
    let events = agent_threads_app::list_project_task_board_item_events(
        &app_state,
        deployment_id,
        params.item_id,
    )
    .await?;
    Ok(events.into())
}

pub async fn list_project_task_board_item_assignments(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<BoardItemParams>,
) -> ApiResult<Vec<ProjectTaskBoardItemAssignment>> {
    let assignments = agent_threads_app::list_project_task_board_item_assignments(
        &app_state,
        deployment_id,
        params.item_id,
    )
    .await?;
    Ok(assignments.into())
}

pub async fn list_project_task_board_item_relations(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<BoardItemParams>,
) -> ApiResult<Vec<ProjectTaskBoardItemRelation>> {
    let relations = agent_threads_app::list_project_task_board_item_relations(
        &app_state,
        deployment_id,
        params.item_id,
    )
    .await?;
    Ok(relations.into())
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
) -> ApiResult<Vec<ThreadEvent>> {
    let events =
        agent_threads_app::list_pending_thread_events(&app_state, deployment_id, params.thread_id)
            .await?;
    Ok(events.into())
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
) -> ApiResult<Vec<ProjectTaskBoardItemAssignment>> {
    let assignments =
        agent_threads_app::list_assignments_for_thread(&app_state, deployment_id, params.thread_id)
            .await?;
    Ok(assignments.into())
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
) -> ApiResult<Vec<ThreadTaskNode>> {
    let nodes = agent_threads_app::list_thread_task_nodes(
        &app_state,
        deployment_id,
        params.graph_id,
        query.include_terminal.unwrap_or(true),
    )
    .await?;
    Ok(nodes.into())
}

pub async fn list_thread_task_edges(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ThreadTaskGraphParams>,
) -> ApiResult<Vec<ThreadTaskEdge>> {
    let edges =
        agent_threads_app::list_thread_task_edges(&app_state, deployment_id, params.graph_id)
            .await?;
    Ok(edges.into())
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
