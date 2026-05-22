use common::ResultExt;
use commands::event_log::{self, EVENT_LOG_WORK_SUBJECT, EventLogPayload, InsertEventLogCommand};
use commands::{
    CreateProjectTaskBoardItemAssignmentCommand, CreateProjectTaskBoardItemCommand,
    EnsureProjectTaskBoardCommand, SetBoardItemPendingApprovalCommand,
    SetBoardItemPendingQuestionCommand,
};
use common::HasDbRouter;
use common::error::AppError;
use dto::json::ask_user::{AnswerSubmission, validate_answers};
use models::{
    ConversationContent, ProjectTaskBoardItem, ProjectTaskBoardItemComment, ToolApprovalMode,
    ToolApprovalRequestState,
};
use queries::{
    GetAgentThreadStateQuery, GetProjectTaskBoardByProjectIdQuery,
    GetProjectTaskBoardItemByIdQuery, ListProjectTaskBoardItemCommentsQuery,
    ResolveThreadExecutionAgentQuery,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::application::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalSubmissionItem {
    pub tool_name: String,
    pub mode: ToolApprovalMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalSubmission {
    pub request_message_id: String,
    pub approvals: Vec<ApprovalSubmissionItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCommentRequest {
    pub body: String,
}

async fn fetch_item(
    app_state: &AppState,
    project_id: i64,
    item_id: i64,
) -> Result<ProjectTaskBoardItem, AppError> {
    let item = GetProjectTaskBoardItemByIdQuery::new(item_id)
        .execute_with_db(app_state.db_router.writer())
        .await?
        .ok_or_else(|| AppError::NotFound("board item not found".to_string()))?;

    let board_project_id = queries::GetProjectTaskBoardProjectIdQuery::new(item.board_id)
        .execute_with_db(app_state.db_router.writer())
        .await?
        .ok_or_else(|| AppError::NotFound("board not found".to_string()))?;
    if board_project_id != project_id {
        return Err(AppError::NotFound("board item not found".to_string()));
    }
    Ok(item)
}

pub async fn cancel_project_task_board_item(
    app_state: &AppState,
    project_id: i64,
    item_id: i64,
) -> Result<ProjectTaskBoardItem, AppError> {
    let item = fetch_item(app_state, project_id, item_id).await?;
    if item.status == "cancelled" || item.status == "completed" {
        return Ok(item);
    }
    let mut tx = app_state.db_router.writer_pool().begin().await?;

    commands::CancelBoardItemCommand { item_id }
        .execute_with_db(&mut *tx)
        .await?;

    commands::CancelAssignmentsForBoardItemCommand { item_id }
        .execute_with_db(&mut *tx)
        .await?;

    tx.commit().await?;

    let updated = GetProjectTaskBoardItemByIdQuery::new(item_id)
        .execute_with_db(app_state.db_router.writer())
        .await?
        .ok_or_else(|| AppError::NotFound("board item not found".to_string()))?;
    Ok(updated)
}

pub async fn answer_project_task_board_item_question(
    app_state: &AppState,
    project_id: i64,
    item_id: i64,
    submission: AnswerSubmission,
) -> Result<ProjectTaskBoardItem, AppError> {
    let item = fetch_item(app_state, project_id, item_id).await?;
    let pending_value = item
        .pending_question
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("no pending question on this task".to_string()))?;
    let pending: models::PendingQuestion = serde_json::from_value(pending_value.clone())
        .map_err(|e| AppError::BadRequest(format!("malformed pending_question: {e}")))?;
    validate_answers(&pending, &submission).map_err(AppError::BadRequest)?;

    let assignment_id = pending.asked_by_assignment_id.ok_or_else(|| {
        AppError::BadRequest(
            "pending question has no assignment context; cannot resume".to_string(),
        )
    })?;

    let assignment = queries::GetAssignmentResumeContextQuery::new(assignment_id)
        .execute_with_db(app_state.db_router.writer())
        .await?
        .ok_or_else(|| AppError::NotFound("assignment not found".to_string()))?;

    let freeform_text = submission.freeform_trimmed();
    let answers_json = serde_json::to_value(&submission.answers)
        .map_err_internal("serialize answers")?;
    let response_content = ConversationContent::ClarificationResponse {
        request_message_id: None,
        answers: answers_json.clone(),
        freeform_text: freeform_text.clone(),
    };
    let conv_id = app_state.sf.next_id()? as i64;

    let mut tx = app_state.db_router.writer_pool().begin().await?;

    SetBoardItemPendingQuestionCommand {
        board_item_id: item_id,
        pending_question: None,
    }
    .execute_with_db(&mut *tx)
    .await?;

    commands::ClearThreadPendingQuestionCommand {
        thread_id: pending.asked_by_thread_id,
    }
    .execute_with_db(&mut *tx)
    .await?;

    commands::CreateConversationCommand::new(
        conv_id,
        pending.asked_by_thread_id,
        response_content.clone(),
        models::ConversationMessageType::ClarificationResponse,
    )
    .with_board_item_id(item_id)
    .execute_with_db(&mut *tx)
    .await?;

    let event_log_id = app_state.sf.next_id()? as i64;
    let mut builder = EventLogPayload::new(
        event_log_id,
        0,
        assignment.thread_id,
        "assignment_execution",
    )
    .with_id("assignment_id", assignment_id)
    .with_id("board_item_id", assignment.board_item_id)
    .with("summary", "User answered the pending clarification; resume the assignment.")
    .with("answers", answers_json);
    if let Some(text) = freeform_text.as_ref() {
        builder = builder.with("freeform_text", text.clone());
    }
    let payload = builder.build();
    InsertEventLogCommand::new(
        event_log_id,
        item.board_id,
        "assignment",
        assignment_id,
        "assignment_execution",
        format!("assignment_execution_{assignment_id}_resume_{event_log_id}"),
    )
    .with_payload(payload)
    .with_priority(20)
    .with_publish_subject(EVENT_LOG_WORK_SUBJECT)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    event_log::nudge_dispatcher(&app_state.nats_client).await;

    GetProjectTaskBoardItemByIdQuery::new(item_id)
        .execute_with_db(app_state.db_router.writer())
        .await?
        .ok_or_else(|| AppError::NotFound("board item not found".to_string()))
}

pub async fn approve_project_task_board_item_tool(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
    item_id: i64,
    submission: ApprovalSubmission,
) -> Result<ProjectTaskBoardItem, AppError> {
    let item = fetch_item(app_state, project_id, item_id).await?;
    let pending_value = item
        .pending_approval
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("no pending approval on this task".to_string()))?;
    let pending: ToolApprovalRequestState = serde_json::from_value(pending_value.clone())
        .map_err(|e| AppError::BadRequest(format!("malformed pending_approval: {e}")))?;
    let request_message_id = pending.request_message_id.as_deref().ok_or_else(|| {
        AppError::BadRequest("pending approval has no request_message_id".to_string())
    })?;
    if request_message_id != submission.request_message_id {
        return Err(AppError::BadRequest(
            "request_message_id does not match the pending approval".to_string(),
        ));
    }

    let mut tool_id_by_name = std::collections::HashMap::<String, i64>::new();
    for tool in &pending.tools {
        tool_id_by_name.insert(tool.tool_name.clone(), tool.tool_id);
    }
    let mut seen = std::collections::HashSet::new();
    for approval in &submission.approvals {
        if approval.tool_name.is_empty() {
            return Err(AppError::BadRequest(
                "approval tool name must be non-empty".to_string(),
            ));
        }
        if !seen.insert(approval.tool_name.clone()) {
            return Err(AppError::BadRequest(format!(
                "duplicate approval for tool '{}'",
                approval.tool_name
            )));
        }
        if !tool_id_by_name.contains_key(&approval.tool_name) {
            return Err(AppError::BadRequest(format!(
                "approval response contains tool '{}' outside the pending approval",
                approval.tool_name
            )));
        }
    }

    let request_message_id_i64: i64 = submission
        .request_message_id
        .parse()
        .map_err(|_| AppError::BadRequest("invalid request_message_id".to_string()))?;
    let asker_thread_id = queries::GetApprovalRequestThreadIdQuery::new(request_message_id_i64)
        .execute_with_db(app_state.db_router.writer())
        .await?
        .ok_or_else(|| AppError::NotFound("approval request conversation not found".to_string()))?;

    let assignment =
        queries::GetActiveAssignmentForThreadOnItemQuery::new(item_id, asker_thread_id)
            .execute_with_db(app_state.db_router.writer())
            .await?
            .ok_or_else(|| {
                AppError::NotFound(
                    "no active assignment found for the approval request".to_string(),
                )
            })?;

    let conv_id = app_state.sf.next_id()? as i64;
    let response_content = ConversationContent::ApprovalResponse {
        request_message_id: Some(request_message_id_i64),
        approvals: submission
            .approvals
            .iter()
            .map(|a| models::ToolApprovalDecision {
                tool_name: a.tool_name.clone(),
                mode: a.mode,
            })
            .collect(),
    };

    let mut tx = app_state.db_router.writer_pool().begin().await?;

    commands::CreateConversationCommand::new(
        conv_id,
        asker_thread_id,
        response_content,
        models::ConversationMessageType::ApprovalResponse,
    )
    .with_board_item_id(item_id)
    .execute_with_db(&mut *tx)
    .await?;

    for approval in &submission.approvals {
        let tool_id = tool_id_by_name[&approval.tool_name];
        let grant_id = app_state.sf.next_id()? as i64;
        commands::InsertApprovalGrantInTxCommand {
            id: grant_id,
            deployment_id,
            thread_id: asker_thread_id,
            tool_id,
            mode: approval.mode,
        }
        .execute_with_db(&mut *tx)
        .await?;
    }

    SetBoardItemPendingApprovalCommand {
        board_item_id: item_id,
        pending_approval: None,
    }
    .execute_with_db(&mut *tx)
    .await?;

    commands::ClearThreadPendingApprovalCommand {
        thread_id: asker_thread_id,
    }
    .execute_with_db(&mut *tx)
    .await?;

    let event_log_id = app_state.sf.next_id()? as i64;
    let payload = EventLogPayload::new(
        event_log_id,
        deployment_id,
        assignment.thread_id,
        "assignment_execution",
    )
    .with_id("assignment_id", assignment.id)
    .with_id("board_item_id", assignment.board_item_id)
    .with("summary", "User responded to the pending approval; resume the assignment.")
    .with_serializable("approvals", submission.approvals)
    .build();
    InsertEventLogCommand::new(
        event_log_id,
        deployment_id,
        "assignment",
        assignment.id,
        "assignment_execution",
        format!(
            "assignment_execution_{}_resume_approval_{}",
            assignment.id, submission.request_message_id
        ),
    )
    .with_payload(payload)
    .with_priority(20)
    .with_publish_subject(EVENT_LOG_WORK_SUBJECT)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    event_log::nudge_dispatcher(&app_state.nats_client).await;

    GetProjectTaskBoardItemByIdQuery::new(item_id)
        .execute_with_db(app_state.db_router.writer())
        .await?
        .ok_or_else(|| AppError::NotFound("board item not found".to_string()))
}

pub async fn list_project_task_board_item_comments(
    app_state: &AppState,
    project_id: i64,
    item_id: i64,
) -> Result<Vec<ProjectTaskBoardItemComment>, AppError> {
    fetch_item(app_state, project_id, item_id).await?;
    ListProjectTaskBoardItemCommentsQuery::new(item_id)
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn create_project_task_board_item_comment(
    app_state: &AppState,
    deployment_id: i64,
    actor_id: i64,
    project_id: i64,
    item_id: i64,
    body: String,
    attachments: Vec<crate::application::agent_threads::WorkspaceUploadInput>,
) -> Result<ProjectTaskBoardItemComment, AppError> {
    let body = body.trim().to_string();
    if body.is_empty() && attachments.is_empty() {
        return Err(AppError::BadRequest(
            "comment body or attachments required".to_string(),
        ));
    }
    let item = fetch_item(app_state, project_id, item_id).await?;

    let uploaded = crate::application::agent_threads::upload_task_workspace_files(
        app_state,
        deployment_id,
        project_id,
        &item.task_key,
        attachments,
    )
    .await?;
    let metadata =
        crate::application::agent_threads::merge_attachments_into_metadata(&json!({}), &uploaded)?;

    let coordinator_thread_id = queries::GetProjectCoordinatorThreadIdQuery::new(project_id)
        .execute_with_db(app_state.db_router.writer())
        .await?;

    let comment_id = app_state.sf.next_id()? as i64;

    let mut tx = app_state.db_router.writer_pool().begin().await?;

    let comment = commands::CreateBoardItemCommentCommand {
        id: comment_id,
        deployment_id,
        board_item_id: item_id,
        actor_id,
        body,
        metadata,
    }
    .execute_with_db(&mut *tx)
    .await?;

    commands::preempt_active_board_item_assignments(
        &mut *tx,
        item_id,
        "Preempted by user comment.",
    )
    .await?;

    commands::ClearBoardItemPendingFlagsCommand { item_id }
        .execute_with_db(&mut *tx)
        .await?;

    if let Some(coord_thread_id) = coordinator_thread_id {
        let event_log_id = app_state.sf.next_id()? as i64;
        let payload = EventLogPayload::new(
            event_log_id,
            deployment_id,
            coord_thread_id,
            "task_routing",
        )
        .with_id("board_item_id", item_id)
        .with("routing_reason", "user_feedback")
        .with("title", item.title.clone())
        .build();
        InsertEventLogCommand::new(
            event_log_id,
            deployment_id,
            "thread",
            coord_thread_id,
            "task_routing",
            format!("task_routing_{coord_thread_id}_{item_id}_{event_log_id}"),
        )
        .with_payload(payload)
        .with_priority(30)
        .with_publish_subject(EVENT_LOG_WORK_SUBJECT)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    if coordinator_thread_id.is_some() {
        event_log::nudge_dispatcher(&app_state.nats_client).await;
    }

    Ok(comment)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateTaskRequest {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub target_lane_thread_id: i64,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub capability_tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateTaskResponse {
    pub task_key: String,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub board_item_id: i64,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub target_lane_thread_id: i64,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub assigned_agent_id: i64,
}

pub async fn delegate_task(
    app_state: &AppState,
    deployment_id: i64,
    project_id: i64,
    request: DelegateTaskRequest,
) -> Result<DelegateTaskResponse, AppError> {
    let title = request.title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::BadRequest(
            "delegate_task requires a non-empty title".to_string(),
        ));
    }

    let lane = GetAgentThreadStateQuery::new(request.target_lane_thread_id, deployment_id)
        .execute_with_db(
            app_state
                .db_router
                .reader(common::db_router::ReadConsistency::Strong),
        )
        .await?;
    if lane.project_id != project_id {
        return Err(AppError::BadRequest(
            "delegate_task: target lane is not in this project".to_string(),
        ));
    }
    if lane.thread_purpose != models::agent_thread::purpose::EXECUTION {
        return Err(AppError::BadRequest(format!(
            "delegate_task: target thread {} is not an execution lane (purpose={})",
            request.target_lane_thread_id, lane.thread_purpose
        )));
    }

    let lane_agent_id =
        ResolveThreadExecutionAgentQuery::new(request.target_lane_thread_id, deployment_id)
            .execute_with_db(
                app_state
                    .db_router
                    .reader(common::db_router::ReadConsistency::Strong),
            )
            .await?
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "delegate_task: lane thread {} has no assigned agent",
                    request.target_lane_thread_id
                ))
            })?;

    let project_thread = GetAgentThreadStateQuery::new(lane.id, deployment_id)
        .execute_with_db(
            app_state
                .db_router
                .reader(common::db_router::ReadConsistency::Strong),
        )
        .await?;
    let actor_id = project_thread.actor_id;

    let board = GetProjectTaskBoardByProjectIdQuery::new(project_id, deployment_id)
        .execute_with_db(app_state.db_router.writer())
        .await?;
    let board = match board {
        Some(board) => board,
        None => {
            EnsureProjectTaskBoardCommand::new(
                app_state.sf.next_id()? as i64,
                deployment_id,
                actor_id,
                project_id,
                format!("Project {} Task Board", project_id),
                "active".to_string(),
            )
            .execute_with_db(app_state.db_router.writer())
            .await?
        }
    };

    let board_item_id = app_state.sf.next_id()? as i64;
    let task_key = format!("DELEGATE-{board_item_id}");

    let mut tx = app_state.db_router.writer().begin().await?;
    let board_item = CreateProjectTaskBoardItemCommand {
        id: board_item_id,
        board_id: board.id,
        task_key: task_key.clone(),
        title,
        description: request.description.clone(),
        status: "pending".to_string(),
        assigned_thread_id: Some(request.target_lane_thread_id),
        metadata: json!({
            "kind": "delegated_task",
            "source": "backend_api",
            "capability_tags": request.capability_tags.clone().unwrap_or_default(),
        }),
        mounts: json!([]),
        exclusive_owner_agent_id: Some(lane_agent_id),
    }
    .execute_with_db(&mut *tx)
    .await?;
    tx.commit().await?;

    let assignment_id = app_state.sf.next_id()? as i64;
    let deps = common::deps::from_app(app_state).db().nats().id();
    CreateProjectTaskBoardItemAssignmentCommand {
        id: assignment_id,
        board_item_id: board_item.id,
        thread_id: request.target_lane_thread_id,
        assignment_role: models::project_task_board::assignment_role::EXECUTOR.to_string(),
        status: models::project_task_board::assignment_status::AVAILABLE.to_string(),
        instructions: request.description,
        metadata: json!({
            "kind": "delegated_task_assignment",
            "source": "backend_api",
        }),
    }
    .execute_with_deps(&deps)
    .await?;

    Ok(DelegateTaskResponse {
        task_key: board_item.task_key,
        board_item_id: board_item.id,
        target_lane_thread_id: request.target_lane_thread_id,
        assigned_agent_id: lane_agent_id,
    })
}
