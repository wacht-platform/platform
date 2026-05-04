use chrono::Utc;
use commands::event_log::{EVENT_LOG_WORK_SUBJECT, InsertEventLogCommand};
use commands::{SetBoardItemPendingApprovalCommand, SetBoardItemPendingQuestionCommand};
use common::error::AppError;
use common::HasDbRouter;
use dto::json::ask_user::{AnswerSubmission, validate_answers};
use models::{
    ConversationContent, ProjectTaskBoardItem, ProjectTaskBoardItemComment, ToolApprovalMode,
    ToolApprovalRequestState,
};
use queries::{GetProjectTaskBoardItemByIdQuery, ListProjectTaskBoardItemCommentsQuery};
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

    let board_row = sqlx::query!(
        r#"SELECT project_id FROM project_task_boards WHERE id = $1"#,
        item.board_id,
    )
    .fetch_optional(app_state.db_router.writer())
    .await?
    .ok_or_else(|| AppError::NotFound("board not found".to_string()))?;
    if board_row.project_id != project_id {
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
    let now = Utc::now();
    let mut tx = app_state.db_router.writer_pool().begin().await?;

    sqlx::query!(
        r#"
        UPDATE project_task_board_items
        SET status = 'cancelled',
            completed_at = $2,
            pending_question = NULL,
            pending_approval = NULL,
            updated_at = $2
        WHERE id = $1
        "#,
        item_id,
        now,
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        r#"
        UPDATE project_task_board_item_assignments
        SET status = 'cancelled',
            result_status = 'task_cancelled',
            result_summary = 'Task cancelled by user.',
            completed_at = $2,
            updated_at = $2
        WHERE board_item_id = $1
          AND status IN ('pending', 'available', 'blocked', 'claimed', 'in_progress')
        "#,
        item_id,
        now,
    )
    .execute(&mut *tx)
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

    let assignment = sqlx::query!(
        r#"
        SELECT thread_id, board_item_id, state_version
        FROM project_task_board_item_assignments
        WHERE id = $1
        "#,
        assignment_id,
    )
    .fetch_optional(app_state.db_router.writer())
    .await?
    .ok_or_else(|| AppError::NotFound("assignment not found".to_string()))?;

    let answers_json = serde_json::to_value(&submission.answers)
        .map_err(|e| AppError::Internal(format!("serialize answers: {e}")))?;
    let response_content = ConversationContent::ClarificationResponse {
        request_message_id: None,
        answers: answers_json.clone(),
    };
    let conv_id = app_state.sf.next_id()? as i64;
    let now = Utc::now();

    let mut tx = app_state.db_router.writer_pool().begin().await?;

    SetBoardItemPendingQuestionCommand {
        board_item_id: item_id,
        pending_question: None,
    }
    .execute_with_db(&mut *tx)
    .await?;

    sqlx::query!(
        r#"
        UPDATE agent_threads
        SET execution_state = jsonb_set(
            COALESCE(execution_state, '{}'::jsonb),
            '{pending_question}',
            'null'::jsonb,
            true
        )
        WHERE id = $1
        "#,
        pending.asked_by_thread_id,
    )
    .execute(&mut *tx)
    .await?;

    let response_json = serde_json::to_value(&response_content)
        .map_err(|e| AppError::Internal(format!("serialize response: {e}")))?;
    sqlx::query!(
        r#"
        INSERT INTO conversations (
            id, thread_id, board_item_id, execution_run_id, timestamp, content, message_type,
            created_at, updated_at, metadata
        ) VALUES (
            $1, $2, $3, NULL, $4, $5::jsonb, 'clarification_response',
            $4, $4, NULL
        )
        "#,
        conv_id,
        pending.asked_by_thread_id,
        item_id,
        now,
        response_json,
    )
    .execute(&mut *tx)
    .await?;

    let event_log_id = app_state.sf.next_id()? as i64;
    let payload = json!({
        "event_log_id": event_log_id.to_string(),
        "deployment_id": "0",
        "thread_id": assignment.thread_id.to_string(),
        "assignment_id": assignment_id.to_string(),
        "board_item_id": assignment.board_item_id.to_string(),
        "kind": "assignment_execution",
        "summary": "User answered the pending clarification; resume the assignment.",
        "answers": answers_json,
    });
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
    let request_message_id = pending
        .request_message_id
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("pending approval has no request_message_id".to_string()))?;
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
            return Err(AppError::BadRequest("approval tool name must be non-empty".to_string()));
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
    let request_conv = sqlx::query!(
        r#"SELECT thread_id FROM conversations WHERE id = $1 AND message_type = 'approval_request'"#,
        request_message_id_i64,
    )
    .fetch_optional(app_state.db_router.writer())
    .await?
    .ok_or_else(|| AppError::NotFound("approval request conversation not found".to_string()))?;
    let asker_thread_id = request_conv
        .thread_id
        .ok_or_else(|| AppError::Internal("approval request has no thread_id".to_string()))?;

    let assignment = sqlx::query!(
        r#"
        SELECT id, thread_id, board_item_id
        FROM project_task_board_item_assignments
        WHERE board_item_id = $1
          AND thread_id = $2
          AND status IN ('claimed', 'in_progress')
        ORDER BY created_at DESC
        LIMIT 1
        "#,
        item_id,
        asker_thread_id,
    )
    .fetch_optional(app_state.db_router.writer())
    .await?
    .ok_or_else(|| {
        AppError::NotFound("no active assignment found for the approval request".to_string())
    })?;

    let now = Utc::now();
    let conv_id = app_state.sf.next_id()? as i64;
    let response_content = json!({
        "type": "approval_response",
        "request_message_id": submission.request_message_id,
        "approvals": submission.approvals,
    });

    let mut tx = app_state.db_router.writer_pool().begin().await?;

    sqlx::query!(
        r#"
        INSERT INTO conversations (
            id, thread_id, board_item_id, execution_run_id, timestamp, content, message_type,
            created_at, updated_at, metadata
        ) VALUES (
            $1, $2, $3, NULL, $4, $5::jsonb, 'approval_response',
            $4, $4, NULL
        )
        "#,
        conv_id,
        asker_thread_id,
        item_id,
        now,
        response_content,
    )
    .execute(&mut *tx)
    .await?;

    for approval in &submission.approvals {
        let tool_id = tool_id_by_name[&approval.tool_name];
        let scope = match approval.mode {
            ToolApprovalMode::AllowOnce => "once",
            ToolApprovalMode::AllowAlways => "thread",
        };
        let grant_id = app_state.sf.next_id()? as i64;
        sqlx::query!(
            r#"
            INSERT INTO approval_grants (
                id, deployment_id, policy_id, actor_id, project_id, thread_id, tool_id,
                granted_by_message_id, grant_scope, status, granted_at, expires_at,
                consumed_at, consumed_by_run_id, metadata
            ) VALUES (
                $1, $2, NULL, NULL, NULL, $3, $4, NULL, $5, 'active', $6, NULL, NULL, NULL,
                '{}'::jsonb
            )
            "#,
            grant_id,
            deployment_id,
            asker_thread_id,
            tool_id,
            scope,
            now,
        )
        .execute(&mut *tx)
        .await?;
    }

    SetBoardItemPendingApprovalCommand {
        board_item_id: item_id,
        pending_approval: None,
    }
    .execute_with_db(&mut *tx)
    .await?;

    sqlx::query!(
        r#"
        UPDATE agent_threads
        SET execution_state = jsonb_set(
            COALESCE(execution_state, '{}'::jsonb),
            '{pending_approval_request}',
            'null'::jsonb,
            true
        )
        WHERE id = $1
        "#,
        asker_thread_id,
    )
    .execute(&mut *tx)
    .await?;

    let event_log_id = app_state.sf.next_id()? as i64;
    let payload = json!({
        "event_log_id": event_log_id.to_string(),
        "deployment_id": deployment_id.to_string(),
        "thread_id": assignment.thread_id.to_string(),
        "assignment_id": assignment.id.to_string(),
        "board_item_id": assignment.board_item_id.to_string(),
        "kind": "assignment_execution",
        "summary": "User responded to the pending approval; resume the assignment.",
        "approvals": submission.approvals,
    });
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
) -> Result<ProjectTaskBoardItemComment, AppError> {
    let body = body.trim().to_string();
    if body.is_empty() {
        return Err(AppError::BadRequest("comment body required".to_string()));
    }
    let item = fetch_item(app_state, project_id, item_id).await?;

    let coordinator_thread_id = sqlx::query!(
        r#"SELECT coordinator_thread_id FROM actor_projects WHERE id = $1"#,
        project_id,
    )
    .fetch_optional(app_state.db_router.writer())
    .await?
    .and_then(|r| r.coordinator_thread_id);

    let comment_id = app_state.sf.next_id()? as i64;
    let now = Utc::now();
    let metadata = json!({});

    let mut tx = app_state.db_router.writer_pool().begin().await?;

    let comment = sqlx::query_as!(
        ProjectTaskBoardItemComment,
        r#"
        INSERT INTO project_task_board_item_comments (
            id, deployment_id, board_item_id, actor_id, body, metadata,
            created_at, updated_at, archived_at, resolved_at, resolved_by_thread_id, resolution_summary
        ) VALUES (
            $1, $2, $3, $4, $5, $6::jsonb, $7, $7, NULL, NULL, NULL, NULL
        )
        RETURNING
            id AS "id!",
            deployment_id AS "deployment_id!",
            board_item_id AS "board_item_id!",
            actor_id AS "actor_id!",
            body AS "body!",
            metadata AS "metadata!",
            created_at AS "created_at!",
            updated_at AS "updated_at!",
            archived_at,
            resolved_at,
            resolved_by_thread_id,
            resolution_summary
        "#,
        comment_id,
        deployment_id,
        item_id,
        actor_id,
        body,
        metadata,
        now,
    )
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query!(
        r#"
        UPDATE project_task_board_item_assignments
        SET status = 'cancelled',
            result_status = 'preempted',
            completed_at = $2,
            result_summary = 'Preempted by user comment.',
            updated_at = $2
        WHERE board_item_id = $1
          AND status IN ('claimed', 'in_progress')
        "#,
        item_id,
        now,
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        r#"
        UPDATE project_task_board_items
        SET pending_question = NULL,
            pending_approval = NULL,
            updated_at = $2
        WHERE id = $1
        "#,
        item_id,
        now,
    )
    .execute(&mut *tx)
    .await?;

    if let Some(coord_thread_id) = coordinator_thread_id {
        let event_log_id = app_state.sf.next_id()? as i64;
        let payload = json!({
            "event_log_id": event_log_id.to_string(),
            "deployment_id": deployment_id.to_string(),
            "thread_id": coord_thread_id.to_string(),
            "board_item_id": item_id.to_string(),
            "kind": "task_routing",
            "routing_reason": "user_feedback",
            "title": item.title,
        });
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

    Ok(comment)
}
