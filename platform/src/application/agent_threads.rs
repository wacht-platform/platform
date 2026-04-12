use commands::{
    CreateActorCommand, CreateActorProjectCommand, CreateAgentThreadCommand,
    CreateProjectTaskBoardItemEventCommand, DispatchThreadEventCommand, EnqueueThreadEventCommand,
};
use common::ReadConsistency;
use common::error::AppError;
use dto::json::deployment::{
    CreateActorProjectRequest, CreateActorRequest, CreateAgentThreadRequest, ExecuteAgentRequest,
    ExecuteAgentResponse,
};
use models::{
    Actor, ActorProject, AgentThread, AgentThreadState, ProjectTaskBoard, ProjectTaskBoardItem,
    ProjectTaskBoardItemAssignment, ProjectTaskBoardItemEvent, ProjectTaskBoardItemRelation,
    ThreadEvent, ThreadTaskEdge, ThreadTaskGraph, ThreadTaskGraphSummary, ThreadTaskNode,
};
use queries::{
    GetActorByIdQuery, GetActorProjectByIdQuery, GetAgentThreadByIdQuery, GetAgentThreadStateQuery,
    GetLatestThreadTaskGraphQuery, GetProjectTaskBoardByIdQuery,
    GetProjectTaskBoardByProjectIdQuery, GetProjectTaskBoardItemByIdQuery, GetThreadEventByIdQuery,
    GetThreadTaskGraphByIdQuery, GetThreadTaskGraphSummaryQuery, ListActorProjectsQuery,
    ListActorsQuery, ListAgentThreadsQuery, ListAssignmentsForThreadQuery,
    ListPendingThreadEventsQuery, ListProjectTaskBoardItemAssignmentsQuery,
    ListProjectTaskBoardItemEventsQuery, ListProjectTaskBoardItemRelationsQuery,
    ListProjectTaskBoardItemsQuery, ListThreadTaskEdgesQuery, ListThreadTaskNodesQuery,
};

use crate::application::{AppState, agent_thread_execution as agent_thread_execution_app};

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

pub async fn create_actor_project(
    app_state: &AppState,
    deployment_id: i64,
    actor_id: i64,
    request: CreateActorProjectRequest,
) -> Result<ActorProject, AppError> {
    get_actor_by_id(app_state, deployment_id, actor_id).await?;

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

    command.execute_with_db(app_state.db_router.writer()).await
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
    create_thread
        .execute_with_db(app_state.db_router.writer())
        .await
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
