use crate::{
    assignment_event::{
        build_assignment_execution_summary, build_task_routing_summary, fetch_assignment_siblings,
    },
    event_log::{self, InsertEventLogCommand},
};
use chrono::Utc;
use common::{
    HasDbRouter, HasIdProvider, HasNatsJetStreamProvider, HasNatsProvider, ReadConsistency,
    error::AppError,
};
use models::{
    ProjectTaskBoard, ProjectTaskBoardItem, ProjectTaskBoardItemAssignment,
    ProjectTaskBoardItemRelation,
};
use queries::{GetProjectTaskBoardItemByIdQuery, ListProjectTaskBoardItemAssignmentsQuery};
use sqlx::Row;

pub struct ReopenBoardItemIfClosedCommand {
    pub board_item_id: i64,
}

impl ReopenBoardItemIfClosedCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<Option<String>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let prior: Option<(String,)> = sqlx::query_as(
            r#"
            WITH pre AS (
                SELECT id, status FROM project_task_board_items WHERE id = $1
            )
            UPDATE project_task_board_items AS u
            SET status = 'pending', completed_at = NULL, updated_at = NOW()
            FROM pre
            WHERE u.id = pre.id
              AND pre.status IN ('completed', 'cancelled', 'blocked')
            RETURNING pre.status AS prior_status
            "#,
        )
        .bind(self.board_item_id)
        .fetch_optional(executor)
        .await
        .map_err(AppError::from)?;
        Ok(prior.map(|(s,)| s))
    }
}

fn board_item_status_is_terminal(status: &str) -> bool {
    matches!(status, "completed" | "cancelled")
}

fn map_assignment_fk_error(err: sqlx::Error, thread_id: i64, board_item_id: i64) -> AppError {
    if let sqlx::Error::Database(db_err) = &err {
        if let Some(constraint) = db_err.constraint() {
            match constraint {
                "project_task_board_item_assignments_thread_id_fkey" => {
                    return AppError::BadRequest(format!(
                        "thread_id {thread_id} is not a valid thread. List the available \
                         threads and pick one of those ids before retrying."
                    ));
                }
                "project_task_board_item_assignments_board_item_id_fkey" => {
                    return AppError::BadRequest(format!(
                        "task {board_item_id} does not exist. List the current tasks and \
                         pick a valid task id before retrying."
                    ));
                }
                _ => {}
            }
        }
    }
    AppError::Database(err)
}

pub struct ResolveBoardItemCommentsCommand {
    pub board_item_id: i64,
    pub comment_ids: Vec<i64>,
    pub resolved_by_thread_id: i64,
    pub resolution_summary: String,
}

impl ResolveBoardItemCommentsCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE project_task_board_item_comments
            SET resolved_at = NOW(),
                resolved_by_thread_id = $3,
                resolution_summary = $4,
                updated_at = NOW()
            WHERE board_item_id = $1
              AND id = ANY($2)
              AND archived_at IS NULL
              AND resolved_at IS NULL
            "#,
            self.board_item_id,
            &self.comment_ids,
            self.resolved_by_thread_id,
            self.resolution_summary,
        )
        .execute(executor)
        .await?;
        Ok(())
    }
}

pub struct SetBoardItemPendingQuestionCommand {
    pub board_item_id: i64,
    pub pending_question: Option<models::PendingQuestion>,
}

impl SetBoardItemPendingQuestionCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let value = self
            .pending_question
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| {
                AppError::Internal(format!("failed to serialize pending_question: {e}"))
            })?;
        sqlx::query!(
            r#"
            UPDATE project_task_board_items
            SET pending_question = $2,
                status = CASE
                    WHEN $2::jsonb IS NOT NULL
                         AND status NOT IN ('completed', 'cancelled')
                        THEN 'needs_clarification'
                    WHEN $2::jsonb IS NULL
                         AND status = 'needs_clarification'
                        THEN 'in_progress'
                    ELSE status
                END,
                updated_at = NOW()
            WHERE id = $1
            "#,
            self.board_item_id,
            value,
        )
        .execute(executor)
        .await?;
        Ok(())
    }
}

pub struct SetBoardItemPendingApprovalCommand {
    pub board_item_id: i64,
    pub pending_approval: Option<models::ToolApprovalRequestState>,
}

impl SetBoardItemPendingApprovalCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let value = self
            .pending_approval
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| {
                AppError::Internal(format!("failed to serialize pending_approval: {e}"))
            })?;
        sqlx::query!(
            r#"
            UPDATE project_task_board_items
            SET pending_approval = $2,
                updated_at = NOW()
            WHERE id = $1
            "#,
            self.board_item_id,
            value,
        )
        .execute(executor)
        .await?;
        Ok(())
    }
}

pub(crate) async fn enqueue_assignment_execution_event_with_deps<D>(
    deps: &D,
    assignment: &ProjectTaskBoardItemAssignment,
) -> Result<(), AppError>
where
    D: HasDbRouter + HasIdProvider + HasNatsProvider + ?Sized,
{
    let thread = sqlx::query!(
        r#"SELECT deployment_id FROM agent_threads WHERE id = $1 AND archived_at IS NULL"#,
        assignment.thread_id
    )
    .fetch_optional(deps.reader_pool(ReadConsistency::Strong))
    .await?;

    let Some(thread) = thread else {
        tracing::warn!(
            assignment_id = assignment.id,
            board_item_id = assignment.board_item_id,
            thread_id = assignment.thread_id,
            "skipping assignment_execution enqueue: executor thread missing or archived"
        );
        return Ok(());
    };

    let board_item = queries::GetProjectTaskBoardItemByIdQuery::new(assignment.board_item_id)
        .execute_with_db(deps.reader_pool(ReadConsistency::Strong))
        .await?;
    let summary = if let Some(board_item) = board_item.as_ref() {
        let siblings = fetch_assignment_siblings(deps, assignment.board_item_id).await?;
        let prior = siblings
            .iter()
            .filter(|a| a.id < assignment.id)
            .max_by_key(|a| a.id);
        Some(build_assignment_execution_summary(
            assignment,
            board_item,
            siblings.len(),
            prior,
        ))
    } else {
        None
    };

    let event_id = deps.id_provider().next_id()? as i64;
    let payload = serde_json::json!({
        "event_log_id": event_id.to_string(),
        "deployment_id": thread.deployment_id.to_string(),
        "thread_id": assignment.thread_id.to_string(),
        "assignment_id": assignment.id.to_string(),
        "board_item_id": assignment.board_item_id.to_string(),
        "kind": "assignment_execution",
        "summary": summary,
    });
    let idempotency_key = format!(
        "assignment_execution_{}_{}",
        assignment.id, assignment.state_version
    );

    InsertEventLogCommand::new(
        event_id,
        thread.deployment_id,
        event_log::aggregate_type::ASSIGNMENT,
        assignment.id,
        "assignment_execution",
        idempotency_key,
    )
    .with_payload(payload)
    .with_priority(20)
    .with_publish_subject(event_log::EVENT_LOG_WORK_SUBJECT)
    .execute(deps.writer_pool())
    .await?;

    event_log::nudge_dispatcher(deps.nats_provider()).await;

    Ok(())
}

pub(crate) async fn enqueue_board_item_to_coordinator_with_deps<D>(
    deps: &D,
    board_item: &ProjectTaskBoardItem,
    note: Option<String>,
    caused_by_thread_id: Option<i64>,
    routing_reason: &'static str,
) -> Result<(), AppError>
where
    D: HasDbRouter + HasIdProvider + HasNatsProvider + ?Sized,
{
    let coordinator = sqlx::query!(
        r#"
        SELECT
            p.coordinator_thread_id,
            t.deployment_id
        FROM project_task_boards b
        INNER JOIN actor_projects p
            ON p.id = b.project_id
        INNER JOIN agent_threads t
            ON t.id = p.coordinator_thread_id
           AND t.archived_at IS NULL
        WHERE b.id = $1
          AND p.coordinator_thread_id IS NOT NULL
          AND b.archived_at IS NULL
          AND p.archived_at IS NULL
        "#,
        board_item.board_id,
    )
    .fetch_optional(deps.reader_pool(ReadConsistency::Strong))
    .await?;

    let Some(coordinator) = coordinator else {
        return Ok(());
    };

    let Some(coordinator_thread_id) = coordinator.coordinator_thread_id else {
        return Ok(());
    };

    let siblings = fetch_assignment_siblings(deps, board_item.id).await?;
    let summary = build_task_routing_summary(board_item, siblings.len());
    let event_id = deps.id_provider().next_id()? as i64;
    let idempotency_key = format!(
        "task_routing_{}_{}",
        board_item.id, board_item.state_version
    );

    crate::InsertTaskRoutingEvent {
        event_log_id: event_id,
        deployment_id: coordinator.deployment_id,
        coordinator_thread_id,
        board_item,
        idempotency_key,
        summary,
        note,
        caused_by_event_id: caused_by_thread_id,
        routing_reason,
        previous_status: None,
        changed_fields: Vec::new(),
        last_assignment_result_status: None,
    }
    .execute(deps.writer_pool())
    .await?;

    event_log::nudge_dispatcher(deps.nats_provider()).await;

    Ok(())
}

async fn maybe_ready_parent_after_child_completion_with_deps<D>(
    deps: &D,
    child_item: &ProjectTaskBoardItem,
) -> Result<Option<ProjectTaskBoardItem>, AppError>
where
    D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + HasNatsProvider + ?Sized,
{
    if child_item.status != "completed" {
        return Ok(None);
    }

    let relation = sqlx::query!(
        r#"
        SELECT parent_board_item_id
        FROM project_task_board_item_relations
        WHERE child_board_item_id = $1
          AND relation_type = 'child_of'
        LIMIT 1
        "#,
        child_item.id,
    )
    .fetch_optional(deps.reader_pool(ReadConsistency::Strong))
    .await?;

    let Some(relation) = relation else {
        return Ok(None);
    };

    let mut tx = deps.writer_pool().begin().await?;
    let Some(parent_item) = GetProjectTaskBoardItemByIdQuery::new(relation.parent_board_item_id)
        .execute_with_db(&mut *tx)
        .await?
    else {
        tx.commit().await?;
        return Ok(None);
    };

    if parent_item.status != "waiting_for_children" {
        tx.commit().await?;
        return Ok(None);
    }

    let child_rows = sqlx::query(
        r#"
        SELECT c.task_key, c.status
        FROM project_task_board_item_relations r
        INNER JOIN project_task_board_items c
            ON c.id = r.child_board_item_id
        WHERE r.parent_board_item_id = $1
          AND r.relation_type = 'child_of'
          AND c.archived_at IS NULL
        ORDER BY c.created_at ASC, c.id ASC
        "#,
    )
    .bind(parent_item.id)
    .fetch_all(&mut *tx)
    .await?;

    if child_rows.is_empty()
        || child_rows
            .iter()
            .any(|row| row.get::<String, _>("status") != "completed")
    {
        tx.commit().await?;
        return Ok(None);
    }

    let now = Utc::now();
    let parent_item = sqlx::query_as::<_, ProjectTaskBoardItem>(
        r#"
        UPDATE project_task_board_items
        SET status = 'pending', updated_at = $2
        WHERE id = $1
        RETURNING
            id, board_id, task_key, title, description, status,
            assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at, state_version,
            schedule_id, scheduled_for, fired_at, pending_question, pending_approval, mounts, exclusive_owner_agent_id, deliverables
        "#,
    )
    .bind(parent_item.id)
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    ReconcileProjectTaskBoardItemCommand::new(parent_item.id)
        .with_note("All child tasks completed; scheduler reevaluated parent task".to_string())
        .execute_with_deps(deps)
        .await?;

    Ok(Some(parent_item))
}

pub struct EnsureProjectTaskBoardCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub actor_id: i64,
    pub project_id: i64,
    pub title: String,
    pub status: String,
}

impl EnsureProjectTaskBoardCommand {
    pub fn new(
        id: i64,
        deployment_id: i64,
        actor_id: i64,
        project_id: i64,
        title: String,
        status: String,
    ) -> Self {
        Self {
            id,
            deployment_id,
            actor_id,
            project_id,
            title,
            status,
        }
    }

    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<ProjectTaskBoard, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = acquirer.begin().await?;

        let existing = sqlx::query_as!(
            ProjectTaskBoard,
            r#"
            SELECT
                id, deployment_id, actor_id, project_id, title, status, metadata,
                created_at, updated_at, archived_at
            FROM project_task_boards
            WHERE deployment_id = $1 AND project_id = $2 AND archived_at IS NULL
            ORDER BY updated_at DESC
            LIMIT 1
            FOR UPDATE
            "#,
            self.deployment_id,
            self.project_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(board) = existing {
            tx.commit().await?;
            return Ok(board);
        }

        let now = Utc::now();
        let board = sqlx::query_as!(
            ProjectTaskBoard,
            r#"
            INSERT INTO project_task_boards (
                id, deployment_id, actor_id, project_id, title, status, metadata,
                created_at, updated_at, archived_at
            ) VALUES ($1, $2, $3, $4, $5, $6, '{}'::jsonb, $7, $7, NULL)
            RETURNING
                id, deployment_id, actor_id, project_id, title, status, metadata,
                created_at, updated_at, archived_at
            "#,
            self.id,
            self.deployment_id,
            self.actor_id,
            self.project_id,
            self.title,
            self.status,
            now
        )
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(board)
    }
}

pub struct UpdateProjectTaskBoardItemMountsCommand {
    pub board_id: i64,
    pub task_key: String,
    pub mounts: serde_json::Value,
}

impl UpdateProjectTaskBoardItemMountsCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE project_task_board_items
            SET mounts = $3, updated_at = NOW()
            WHERE board_id = $1 AND task_key = $2 AND archived_at IS NULL
            "#,
            self.board_id,
            self.task_key,
            self.mounts,
        )
        .execute(executor)
        .await?;
        Ok(())
    }
}

pub struct SetProjectTaskBoardItemArchivedCommand {
    pub board_id: i64,
    pub item_id: i64,
    pub archived: bool,
}

impl SetProjectTaskBoardItemArchivedCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ProjectTaskBoardItem, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let item = sqlx::query_as!(
            ProjectTaskBoardItem,
            r#"
            UPDATE project_task_board_items
            SET archived_at = CASE WHEN $3 THEN NOW() ELSE NULL END,
                updated_at = NOW()
            WHERE id = $1 AND board_id = $2
            RETURNING id, board_id, task_key, title, description, status,
                      assigned_thread_id, metadata, completed_at, archived_at,
                      created_at, updated_at, state_version,
                      schedule_id, scheduled_for, fired_at,
                      pending_question, pending_approval, mounts, exclusive_owner_agent_id, deliverables
            "#,
            self.item_id,
            self.board_id,
            self.archived,
        )
        .fetch_one(executor)
        .await?;
        Ok(item)
    }
}

pub struct AttachProjectTaskBoardItemScheduleCommand {
    pub board_id: i64,
    pub task_key: String,
    pub schedule_id: i64,
    pub mounts: serde_json::Value,
}

impl AttachProjectTaskBoardItemScheduleCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ProjectTaskBoardItem, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let item = sqlx::query_as::<_, ProjectTaskBoardItem>(
            r#"
            UPDATE project_task_board_items
            SET schedule_id = $3, mounts = $4, updated_at = NOW()
            WHERE board_id = $1 AND task_key = $2 AND archived_at IS NULL
            RETURNING
                id, board_id, task_key, title, description, status,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at, state_version,
                schedule_id, scheduled_for, fired_at, pending_question, pending_approval, mounts, exclusive_owner_agent_id, deliverables
            "#,
        )
        .bind(self.board_id)
        .bind(self.task_key)
        .bind(self.schedule_id)
        .bind(self.mounts)
        .fetch_one(executor)
        .await?;
        Ok(item)
    }
}

pub struct CreateProjectTaskBoardItemCommand {
    pub id: i64,
    pub board_id: i64,
    pub task_key: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub assigned_thread_id: Option<i64>,
    pub metadata: serde_json::Value,
    pub mounts: serde_json::Value,
    pub exclusive_owner_agent_id: Option<i64>,
}

impl CreateProjectTaskBoardItemCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ProjectTaskBoardItem, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = Utc::now();

        let item = sqlx::query_as::<_, ProjectTaskBoardItem>(
            r#"
            INSERT INTO project_task_board_items (
                id, board_id, task_key, title, description, status,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at, state_version,
            schedule_id, scheduled_for, fired_at, pending_question, pending_approval, mounts, exclusive_owner_agent_id
            ) VALUES (
                $1, $2, $3, $4, $5, $6,
                $7, $8,
                CASE
                    WHEN $6::text = 'completed' THEN $9::timestamptz
                    ELSE NULL::timestamptz
                END,
                NULL,
                $9, $9, 0,
                NULL, NULL, NULL, NULL, NULL,
                $10, $11
            )
            RETURNING
                id, board_id, task_key, title, description, status,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at, state_version,
            schedule_id, scheduled_for, fired_at, pending_question, pending_approval, mounts, exclusive_owner_agent_id, deliverables
            "#,
        )
        .bind(self.id)
        .bind(self.board_id)
        .bind(self.task_key)
        .bind(self.title)
        .bind(self.description)
        .bind(self.status)
        .bind(self.assigned_thread_id)
        .bind(self.metadata)
        .bind(now)
        .bind(self.mounts)
        .bind(self.exclusive_owner_agent_id)
        .fetch_one(executor)
        .await?;

        Ok(item)
    }
}

pub struct CreateProjectTaskBoardItemRelationCommand {
    pub id: i64,
    pub board_id: i64,
    pub parent_board_item_id: i64,
    pub child_board_item_id: i64,
    pub relation_type: String,
    pub metadata: serde_json::Value,
}

impl CreateProjectTaskBoardItemRelationCommand {
    async fn validate_child_of_with_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), AppError> {
        if self.relation_type != models::project_task_board::relation_type::CHILD_OF {
            return Err(AppError::BadRequest(format!(
                "Unsupported task relation type '{}'",
                self.relation_type
            )));
        }

        if self.parent_board_item_id == self.child_board_item_id {
            return Err(AppError::BadRequest(
                "A task cannot be its own parent".to_string(),
            ));
        }

        let parent = sqlx::query_as::<_, ProjectTaskBoardItem>(
            r#"
            SELECT
                id, board_id, task_key, title, description, status,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at, state_version,
            schedule_id, scheduled_for, fired_at, pending_question, pending_approval, mounts, exclusive_owner_agent_id, deliverables
            FROM project_task_board_items
            WHERE id = $1 AND archived_at IS NULL
            LIMIT 1
            "#,
        )
        .bind(self.parent_board_item_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::BadRequest("Parent task was not found".to_string()))?;

        let child = sqlx::query_as::<_, ProjectTaskBoardItem>(
            r#"
            SELECT
                id, board_id, task_key, title, description, status,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at, state_version,
            schedule_id, scheduled_for, fired_at, pending_question, pending_approval, mounts, exclusive_owner_agent_id, deliverables
            FROM project_task_board_items
            WHERE id = $1 AND archived_at IS NULL
            LIMIT 1
            "#,
        )
        .bind(self.child_board_item_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::BadRequest("Child task was not found".to_string()))?;

        if parent.board_id != self.board_id || child.board_id != self.board_id {
            return Err(AppError::BadRequest(
                "Parent and child tasks must belong to the same board".to_string(),
            ));
        }

        let existing_parent = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT parent_board_item_id
            FROM project_task_board_item_relations
            WHERE child_board_item_id = $1
              AND relation_type = $2
            LIMIT 1
            "#,
        )
        .bind(self.child_board_item_id)
        .bind(models::project_task_board::relation_type::CHILD_OF)
        .fetch_optional(&mut **tx)
        .await?;

        if let Some(existing_parent_id) = existing_parent {
            if existing_parent_id == self.parent_board_item_id {
                return Err(AppError::BadRequest(
                    "Child task is already linked to that parent".to_string(),
                ));
            }
            return Err(AppError::BadRequest(
                "Child task already has a parent".to_string(),
            ));
        }

        let mut current_ancestor_id = Some(self.parent_board_item_id);
        while let Some(ancestor_id) = current_ancestor_id {
            if ancestor_id == self.child_board_item_id {
                return Err(AppError::BadRequest(
                    "Task hierarchy would create a cycle".to_string(),
                ));
            }

            current_ancestor_id = sqlx::query_scalar::<_, i64>(
                r#"
                SELECT parent_board_item_id
                FROM project_task_board_item_relations
                WHERE child_board_item_id = $1
                  AND relation_type = $2
                LIMIT 1
                "#,
            )
            .bind(ancestor_id)
            .bind(models::project_task_board::relation_type::CHILD_OF)
            .fetch_optional(&mut **tx)
            .await?;
        }

        Ok(())
    }

    pub async fn execute_with_tx(
        self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<ProjectTaskBoardItemRelation, AppError> {
        self.validate_child_of_with_tx(tx).await?;

        let now = Utc::now();
        let relation = sqlx::query_as::<_, ProjectTaskBoardItemRelation>(
            r#"
            INSERT INTO project_task_board_item_relations (
                id,
                board_id,
                parent_board_item_id,
                child_board_item_id,
                relation_type,
                metadata,
                created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING
                id,
                board_id,
                parent_board_item_id,
                child_board_item_id,
                relation_type,
                metadata,
                created_at
            "#,
        )
        .bind(self.id)
        .bind(self.board_id)
        .bind(self.parent_board_item_id)
        .bind(self.child_board_item_id)
        .bind(self.relation_type)
        .bind(self.metadata)
        .bind(now)
        .fetch_one(&mut **tx)
        .await?;

        Ok(relation)
    }

    pub async fn execute_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<ProjectTaskBoardItemRelation, AppError>
    where
        D: HasDbRouter + ?Sized,
    {
        let mut tx = deps.writer_pool().begin().await?;
        let relation = self.execute_with_tx(&mut tx).await?;
        tx.commit().await?;
        Ok(relation)
    }
}

pub struct UpdateProjectTaskBoardItemCommand {
    pub deployment_id: i64,
    pub board_id: i64,
    pub task_key: String,
    pub status: Option<String>,
    pub metadata: serde_json::Value,
}

impl UpdateProjectTaskBoardItemCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<ProjectTaskBoardItem, AppError>
    where
        D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + HasNatsProvider + ?Sized,
    {
        let deployment_id = self.deployment_id;
        let board_id = self.board_id;
        let task_key = self.task_key.clone();
        let new_status = self.status.clone();

        let mut tx = deps.writer_pool().begin().await?;

        let original_status: Option<String> = sqlx::query_scalar!(
            r#"SELECT status FROM project_task_board_items
               WHERE board_id = $1 AND task_key = $2 AND archived_at IS NULL"#,
            board_id,
            task_key,
        )
        .fetch_optional(&mut *tx)
        .await?;

        let item = self.execute_with_db(&mut *tx).await?;

        let mut subscriptions_fired = false;
        let mut cancelled_now = false;
        if let (Some(prior), Some(_)) = (original_status.as_deref(), new_status.as_deref()) {
            if prior != item.status {
                if let Some(kind) = models::TaskSubscriptionEventKind::from_status(&item.status) {
                    let count = crate::fan_out_task_subscription_notifications(
                        &mut tx,
                        deps,
                        deployment_id,
                        &item,
                        prior,
                        kind,
                        chrono::Utc::now(),
                    )
                    .await?;
                    subscriptions_fired = count > 0;
                }
                if item.status == "cancelled" && prior != "cancelled" {
                    cancelled_now = true;
                }
            }
        }

        if cancelled_now {
            crate::DeleteSubscriptionsForBoardItemCommand {
                board_item_id: item.id,
            }
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        if subscriptions_fired {
            event_log::nudge_dispatcher(deps.nats_provider()).await;
        }

        let _ = maybe_ready_parent_after_child_completion_with_deps(deps, &item).await?;

        Ok(item)
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ProjectTaskBoardItem, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = Utc::now();
        let task_key = self.task_key;

        let item = sqlx::query_as::<_, ProjectTaskBoardItem>(
            r#"
            UPDATE project_task_board_items
            SET
                status = COALESCE($3, status),
                metadata = $4,
                completed_at = CASE
                    WHEN COALESCE($3, status) = 'completed' THEN COALESCE(completed_at, $5)
                    WHEN $3 IS NOT NULL THEN NULL
                    ELSE completed_at
                END,
                updated_at = $5
            WHERE board_id = $1 AND task_key = $2 AND archived_at IS NULL
            RETURNING
                id, board_id, task_key, title, description, status,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at, state_version,
            schedule_id, scheduled_for, fired_at, pending_question, pending_approval, mounts, exclusive_owner_agent_id, deliverables
            "#,
        )
        .bind(self.board_id)
        .bind(&task_key)
        .bind(self.status)
        .bind(self.metadata)
        .bind(now)
        .fetch_optional(executor)
        .await?;

        item.ok_or_else(|| {
            AppError::BadRequest(format!(
                "Project task '{}' was not found in the current board",
                task_key
            ))
        })
    }
}

pub struct ReconcileProjectTaskBoardItemCommand {
    pub board_item_id: i64,
    pub note: Option<String>,
    pub caused_by_thread_id: Option<i64>,
}

impl ReconcileProjectTaskBoardItemCommand {
    pub fn new(board_item_id: i64) -> Self {
        Self {
            board_item_id,
            note: None,
            caused_by_thread_id: None,
        }
    }

    pub fn with_note(mut self, note: String) -> Self {
        self.note = Some(note);
        self
    }

    pub fn with_caused_by_thread_id(mut self, thread_id: i64) -> Self {
        self.caused_by_thread_id = Some(thread_id);
        self
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + HasNatsProvider + ?Sized,
    {
        let Some(board_item) = GetProjectTaskBoardItemByIdQuery::new(self.board_item_id)
            .execute_with_db(deps.reader_pool(ReadConsistency::Strong))
            .await?
        else {
            return Ok(());
        };

        if board_item_status_is_terminal(&board_item.status) {
            return Ok(());
        }

        let assignments = ListProjectTaskBoardItemAssignmentsQuery::new(board_item.id)
            .execute_with_db(deps.reader_pool(ReadConsistency::Strong))
            .await?;

        // Coordinator-role assignments are bookkeeping markers (the
        // coordinator owns the board item during its routing turn); they
        // don't represent execution work, so they must not block dispatch
        // of a freshly-created executor assignment in the same turn.
        if assignments.iter().any(|assignment| {
            matches!(assignment.status.as_str(), "claimed" | "in_progress")
                && assignment.assignment_role
                    != models::project_task_board::assignment_role::COORDINATOR
        }) {
            return Ok(());
        }

        if let Some(assignment) = assignments
            .iter()
            .filter(|assignment| {
                assignment.status == models::project_task_board::assignment_status::AVAILABLE
            })
            .min_by_key(|assignment| assignment.id)
        {
            enqueue_assignment_execution_event_with_deps(deps, assignment).await?;
            return Ok(());
        }

        if let Some(next_assignment) = assignments
            .iter()
            .filter(|assignment| {
                assignment.status == models::project_task_board::assignment_status::PENDING
            })
            .min_by_key(|assignment| assignment.id)
        {
            let activation_note = self.note.clone().unwrap_or_else(|| {
                format!("Scheduler activated assignment {}", next_assignment.id)
            });

            let activated = UpdateProjectTaskBoardItemAssignmentStateCommand::new(
                next_assignment.id,
                models::project_task_board::assignment_status::AVAILABLE.to_string(),
            )
            .with_note(activation_note)
            .without_reconcile()
            .apply_with_deps(deps)
            .await?;

            enqueue_assignment_execution_event_with_deps(deps, &activated).await?;
            return Ok(());
        }

        if board_item.exclusive_owner_agent_id.is_some() {
            auto_complete_agent_owned_task(deps, &board_item, &assignments).await?;
            return Ok(());
        }

        // Skip re-routing to coord if a coord-role turn is already in flight.
        if assignments.iter().any(|a| {
            a.assignment_role == models::project_task_board::assignment_role::COORDINATOR
                && matches!(a.status.as_str(), "claimed" | "in_progress")
        }) {
            return Ok(());
        }

        enqueue_board_item_to_coordinator_with_deps(
            deps,
            &board_item,
            self.note.or_else(|| {
                Some("No active assignment available; returned task to coordinator".to_string())
            }),
            self.caused_by_thread_id,
            models::thread_event::routing_reason::ASSIGNMENT_COMPLETED,
        )
        .await?;

        Ok(())
    }
}

async fn auto_complete_agent_owned_task<D>(
    deps: &D,
    board_item: &ProjectTaskBoardItem,
    assignments: &[ProjectTaskBoardItemAssignment],
) -> Result<(), AppError>
where
    D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + HasNatsProvider + ?Sized,
{
    let latest = assignments
        .iter()
        .filter(|a| {
            matches!(
                a.status.as_str(),
                "completed" | "blocked" | "cancelled" | "rejected"
            )
        })
        .max_by_key(|a| a.updated_at);

    let new_status = match latest.map(|a| a.status.as_str()) {
        Some("completed") => "completed",
        Some("blocked") => "blocked",
        Some("cancelled") | Some("rejected") => "cancelled",
        _ => "completed",
    };

    if board_item.status == new_status {
        return Ok(());
    }

    let deployment_id: i64 = sqlx::query_scalar!(
        r#"SELECT deployment_id FROM project_task_boards WHERE id = $1"#,
        board_item.board_id,
    )
    .fetch_one(deps.writer_pool())
    .await?;

    let original_status = board_item.status.clone();
    let mut tx = deps.writer_pool().begin().await?;
    let updated = sqlx::query_as!(
        ProjectTaskBoardItem,
        r#"
        UPDATE project_task_board_items
        SET status = $2,
            completed_at = CASE WHEN $2 = 'completed' THEN NOW() ELSE completed_at END,
            updated_at = NOW()
        WHERE id = $1 AND archived_at IS NULL
        RETURNING id, board_id, task_key, title, description, status,
                  assigned_thread_id, metadata, completed_at, archived_at,
                  created_at, updated_at, state_version,
                  schedule_id, scheduled_for, fired_at,
                  pending_question, pending_approval, mounts, exclusive_owner_agent_id, deliverables
        "#,
        board_item.id,
        new_status,
    )
    .fetch_one(&mut *tx)
    .await?;

    let mut subscriptions_fired = false;
    if let Some(kind) = models::TaskSubscriptionEventKind::from_status(&updated.status) {
        let count = crate::fan_out_task_subscription_notifications(
            &mut tx,
            deps,
            deployment_id,
            &updated,
            &original_status,
            kind,
            Utc::now(),
        )
        .await?;
        subscriptions_fired = count > 0;
    }

    if new_status == "cancelled" {
        crate::DeleteSubscriptionsForBoardItemCommand {
            board_item_id: updated.id,
        }
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    if subscriptions_fired {
        event_log::nudge_dispatcher(deps.nats_provider()).await;
    }

    Ok(())
}

pub struct CreateProjectTaskBoardItemAssignmentCommand {
    pub id: i64,
    pub board_item_id: i64,
    pub thread_id: i64,
    pub assignment_role: String,
    pub status: String,
    pub instructions: Option<String>,
    pub metadata: serde_json::Value,
}

impl CreateProjectTaskBoardItemAssignmentCommand {
    pub async fn execute_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<ProjectTaskBoardItemAssignment, AppError>
    where
        D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + HasNatsProvider + ?Sized,
    {
        let now = Utc::now();
        let mut tx = deps.writer_pool().begin().await?;
        let assignment = sqlx::query_as!(
            ProjectTaskBoardItemAssignment,
            r#"
            INSERT INTO project_task_board_item_assignments (
                id, board_item_id, thread_id, assignment_role, status,
                instructions, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at
            ) VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, NULL, NULL,
                NULL, NULL, NULL, NULL, NULL, $8, $8
            )
            RETURNING
                id, board_item_id, thread_id, assignment_role, status,
                instructions, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at, state_version
            "#,
            self.id,
            self.board_item_id,
            self.thread_id,
            self.assignment_role,
            self.status,
            self.instructions,
            self.metadata,
            now,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| map_assignment_fk_error(e, self.thread_id, self.board_item_id))?;

        if matches!(
            assignment.status.as_str(),
            models::project_task_board::assignment_status::AVAILABLE
                | models::project_task_board::assignment_status::CLAIMED
                | models::project_task_board::assignment_status::IN_PROGRESS
        ) {
            sqlx::query!(
                r#"
                UPDATE project_task_board_items
                SET assigned_thread_id = $2, updated_at = $3
                WHERE id = $1
                "#,
                assignment.board_item_id,
                assignment.thread_id,
                now,
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        ReconcileProjectTaskBoardItemCommand::new(assignment.board_item_id)
            .with_note("Task assignment created; scheduler reevaluated routing".to_string())
            .execute_with_deps(deps)
            .await?;

        Ok(assignment)
    }
}

pub struct UpdateProjectTaskBoardItemAssignmentCommand {
    pub assignment_id: i64,
    pub thread_id: i64,
    pub assignment_role: String,
    pub status: String,
    pub instructions: Option<String>,
    pub metadata: serde_json::Value,
}

impl UpdateProjectTaskBoardItemAssignmentCommand {
    pub async fn execute_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<ProjectTaskBoardItemAssignment, AppError>
    where
        D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + HasNatsProvider + ?Sized,
    {
        let mut tx = deps.writer_pool().begin().await?;
        let now = Utc::now();

        let current = sqlx::query_as!(
            ProjectTaskBoardItemAssignment,
            r#"
            SELECT
                id, board_item_id, thread_id, assignment_role, status,
                instructions, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at, state_version
            FROM project_task_board_item_assignments
            WHERE id = $1
            FOR UPDATE
            "#,
            self.assignment_id,
        )
        .fetch_one(&mut *tx)
        .await?;

        if matches!(
            current.status.as_str(),
            "claimed" | "in_progress" | "completed" | "rejected"
        ) {
            tx.commit().await?;
            return Ok(current);
        }

        let assignment = sqlx::query_as!(
            ProjectTaskBoardItemAssignment,
            r#"
            UPDATE project_task_board_item_assignments
            SET
                thread_id = $2,
                assignment_role = $3,
                status = $4,
                instructions = $5,
                metadata = $6,
                result_status = CASE
                    WHEN $4 IN ('pending', 'available', 'claimed', 'in_progress') THEN NULL
                    ELSE result_status
                END,
                result_summary = CASE
                    WHEN $4 IN ('pending', 'available', 'claimed', 'in_progress') THEN NULL
                    ELSE result_summary
                END,
                result_payload = CASE
                    WHEN $4 IN ('pending', 'available', 'claimed', 'in_progress') THEN NULL
                    ELSE result_payload
                END,
                updated_at = $7
            WHERE id = $1
            RETURNING
                id, board_item_id, thread_id, assignment_role, status,
                instructions, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at, state_version
            "#,
            self.assignment_id,
            self.thread_id,
            self.assignment_role,
            self.status,
            self.instructions,
            self.metadata,
            now,
        )
        .fetch_one(&mut *tx)
        .await?;

        if matches!(
            assignment.status.as_str(),
            "available" | "claimed" | "in_progress"
        ) {
            sqlx::query!(
                r#"
                UPDATE project_task_board_items
                SET assigned_thread_id = $2, updated_at = $3
                WHERE id = $1
                "#,
                assignment.board_item_id,
                assignment.thread_id,
                now,
            )
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query!(
                r#"
                UPDATE project_task_board_items
                SET assigned_thread_id = CASE
                        WHEN assigned_thread_id = $2 THEN NULL
                        ELSE assigned_thread_id
                    END,
                    updated_at = $3
                WHERE id = $1
                "#,
                assignment.board_item_id,
                current.thread_id,
                now,
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(assignment)
    }
}

/// Complete a coordinator-role assignment when the coordinator's turn
/// ends. Unlike `UpdateProjectTaskBoardItemAssignmentStateCommand`, this
/// preserves `board_item.assigned_thread_id` if it has already been
/// transferred to another thread (e.g. by `assign_project_task` creating
/// an executor assignment in the same coordinator turn). Only blanks the
/// pointer when it still points at the coordinator's own thread.
pub struct MarkCoordinatorAssignmentCompletedCommand {
    pub assignment_id: i64,
    pub coordinator_thread_id: i64,
    pub board_item_id: i64,
}

/// Look up an active coordinator-role assignment for (board_item,
/// thread); create one if none exists. Used by the agent runtime when a
/// TASK_ROUTING event reaches a coordinator thread, so the coordinator
/// is an explicit owner of the board item while it decides routing.
/// Returns the assignment id.
pub struct EnsureCoordinatorAssignmentCommand {
    pub board_item_id: i64,
    pub coordinator_thread_id: i64,
}

impl EnsureCoordinatorAssignmentCommand {
    pub fn new(board_item_id: i64, coordinator_thread_id: i64) -> Self {
        Self {
            board_item_id,
            coordinator_thread_id,
        }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<i64, AppError>
    where
        D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + HasNatsProvider + ?Sized,
    {
        let existing = sqlx::query_scalar!(
            r#"
            SELECT id
            FROM project_task_board_item_assignments
            WHERE board_item_id = $1
              AND thread_id = $2
              AND assignment_role = 'coordinator'
              AND status NOT IN ('completed', 'cancelled', 'rejected')
            ORDER BY id DESC
            LIMIT 1
            "#,
            self.board_item_id,
            self.coordinator_thread_id,
        )
        .fetch_optional(deps.writer_pool())
        .await?;

        if let Some(id) = existing {
            return Ok(id);
        }

        let new_id = deps.id_provider().next_id()? as i64;
        let assignment = CreateProjectTaskBoardItemAssignmentCommand {
            id: new_id,
            board_item_id: self.board_item_id,
            thread_id: self.coordinator_thread_id,
            assignment_role: models::project_task_board::assignment_role::COORDINATOR.to_string(),
            status: models::project_task_board::assignment_status::IN_PROGRESS.to_string(),
            instructions: None,
            metadata: serde_json::json!({}),
        }
        .execute_with_deps(deps)
        .await?;
        Ok(assignment.id)
    }
}

impl MarkCoordinatorAssignmentCompletedCommand {
    pub fn new(assignment_id: i64, coordinator_thread_id: i64, board_item_id: i64) -> Self {
        Self {
            assignment_id,
            coordinator_thread_id,
            board_item_id,
        }
    }

    pub async fn execute_with_db<'e, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'e, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        sqlx::query!(
            r#"
            UPDATE project_task_board_item_assignments
            SET status = 'completed',
                result_status = COALESCE(result_status, 'completed'),
                completed_at = COALESCE(completed_at, NOW()),
                updated_at = NOW()
            WHERE id = $1
              AND status NOT IN ('completed', 'cancelled', 'rejected')
            "#,
            self.assignment_id,
        )
        .execute(&mut *conn)
        .await?;

        sqlx::query!(
            r#"
            UPDATE project_task_board_items
            SET assigned_thread_id = CASE
                    WHEN assigned_thread_id = $2 THEN NULL
                    ELSE assigned_thread_id
                END,
                updated_at = NOW()
            WHERE id = $1
            "#,
            self.board_item_id,
            self.coordinator_thread_id,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}

pub struct WriteAssignmentResultPayloadCommand {
    pub assignment_id: i64,
    pub payload: serde_json::Value,
}

impl WriteAssignmentResultPayloadCommand {
    pub fn new(assignment_id: i64, payload: serde_json::Value) -> Self {
        Self {
            assignment_id,
            payload,
        }
    }

    /// Shallow-merges `payload` into the existing `result_payload` JSONB
    /// rather than overwriting. Other keys on the column (e.g.
    /// `handoff_file_path` read by the agent loop) are preserved.
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE project_task_board_item_assignments
            SET result_payload = COALESCE(result_payload, '{}'::jsonb) || $2::jsonb,
                updated_at = NOW()
            WHERE id = $1
            "#,
            self.assignment_id,
            self.payload,
        )
        .execute(executor)
        .await?;
        Ok(())
    }
}

pub struct ReplaceBoardItemMetadataCommand {
    pub board_item_id: i64,
    pub metadata: serde_json::Value,
}

impl ReplaceBoardItemMetadataCommand {
    pub fn new(board_item_id: i64, metadata: serde_json::Value) -> Self {
        Self {
            board_item_id,
            metadata,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE project_task_board_items
            SET metadata = $2,
                updated_at = NOW()
            WHERE id = $1 AND archived_at IS NULL
            "#,
            self.board_item_id,
            self.metadata,
        )
        .execute(executor)
        .await?;
        Ok(())
    }
}

pub struct AppendBoardItemDeliverableCommand {
    pub board_item_id: i64,
    pub entry: serde_json::Value,
}

impl AppendBoardItemDeliverableCommand {
    pub fn new(board_item_id: i64, entry: serde_json::Value) -> Self {
        Self {
            board_item_id,
            entry,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE project_task_board_items
            SET deliverables = COALESCE(deliverables, '[]'::jsonb) || jsonb_build_array($2::jsonb),
                updated_at = NOW()
            WHERE id = $1 AND archived_at IS NULL
            "#,
            self.board_item_id,
            self.entry,
        )
        .execute(executor)
        .await?;
        Ok(())
    }
}

pub struct UpdateProjectTaskBoardItemAssignmentStateCommand {
    pub assignment_id: i64,
    pub status: String,
    pub note: Option<String>,
    pub result_status: Option<String>,
    pub result_summary: Option<String>,
    pub result_payload: Option<serde_json::Value>,
    pub suppress_reconcile: bool,
}

impl UpdateProjectTaskBoardItemAssignmentStateCommand {
    pub fn new(assignment_id: i64, status: String) -> Self {
        Self {
            assignment_id,
            status,
            note: None,
            result_status: None,
            result_summary: None,
            result_payload: None,
            suppress_reconcile: false,
        }
    }

    pub fn with_note(mut self, note: String) -> Self {
        self.note = Some(note);
        self
    }

    pub fn with_result(
        mut self,
        result_status: Option<String>,
        result_summary: Option<String>,
        result_payload: Option<serde_json::Value>,
    ) -> Self {
        self.result_status = result_status;
        self.result_summary = result_summary;
        self.result_payload = result_payload;
        self
    }

    pub fn without_reconcile(mut self) -> Self {
        self.suppress_reconcile = true;
        self
    }

    pub async fn execute_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<ProjectTaskBoardItemAssignment, AppError>
    where
        D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + HasNatsProvider + ?Sized,
    {
        let suppress_reconcile = self.suppress_reconcile;
        let custom_note = self.note.clone();
        let assignment = self.apply_with_deps(deps).await?;

        if !suppress_reconcile {
            let reconcile_note = custom_note.unwrap_or_else(|| {
                format!(
                    "Assignment {} moved to {}; scheduler reevaluated routing",
                    assignment.id, assignment.status
                )
            });
            ReconcileProjectTaskBoardItemCommand::new(assignment.board_item_id)
                .with_note(reconcile_note)
                .with_caused_by_thread_id(assignment.thread_id)
                .execute_with_deps(deps)
                .await?;
        }

        Ok(assignment)
    }

    /// State mutation in a single tx. Does NOT trigger reconcile.
    /// `execute_with_deps` is the full path (apply + maybe reconcile);
    /// `ReconcileProjectTaskBoardItemCommand` calls `apply_with_deps`
    /// directly to avoid re-entering itself.
    pub(crate) async fn apply_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<ProjectTaskBoardItemAssignment, AppError>
    where
        D: HasDbRouter + ?Sized,
    {
        let mut tx = deps.writer_pool().begin().await?;
        let now = Utc::now();
        let result_status = self.result_status.clone();
        let result_summary = self.result_summary.clone();
        let result_payload = self.result_payload.clone();

        let assignment = sqlx::query_as!(
            ProjectTaskBoardItemAssignment,
            r#"
            UPDATE project_task_board_item_assignments
            SET
                status = $2,
                result_status = CASE
                    WHEN $3::text IS NOT NULL THEN $3
                    WHEN $2 = 'completed' THEN 'completed'
                    WHEN $2 = 'blocked' THEN 'blocked'
                    WHEN $2 = 'rejected' THEN 'rejected'
                    WHEN $2 = 'cancelled' THEN 'cancelled'
                    ELSE result_status
                END,
                result_summary = COALESCE($4, result_summary),
                result_payload = COALESCE($5, result_payload),
                claimed_at = CASE
                    WHEN $2 = 'claimed' AND claimed_at IS NULL THEN $6
                    ELSE claimed_at
                END,
                started_at = CASE
                    WHEN $2 = 'in_progress' AND started_at IS NULL THEN $6
                    ELSE started_at
                END,
                completed_at = CASE
                    WHEN $2 = 'completed' THEN $6
                    ELSE completed_at
                END,
                rejected_at = CASE
                    WHEN $2 = 'rejected' THEN $6
                    ELSE rejected_at
                END,
                updated_at = $6
            WHERE id = $1
            RETURNING
                id, board_item_id, thread_id, assignment_role, status,
                instructions, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at, state_version
            "#,
            self.assignment_id,
            self.status,
            result_status,
            result_summary,
            result_payload,
            now,
        )
        .fetch_one(&mut *tx)
        .await?;

        let current_board_item = GetProjectTaskBoardItemByIdQuery::new(assignment.board_item_id)
            .execute_with_db(&mut *tx)
            .await?;
        let board_item_already_terminal = current_board_item
            .as_ref()
            .map(|item| board_item_status_is_terminal(&item.status))
            .unwrap_or(false);

        let mut board_item_status: Option<&str> = None;
        let mut board_item_assigned_thread_id: Option<Option<i64>> = None;
        let mut board_item_completed_at: Option<Option<chrono::DateTime<Utc>>> = None;
        if matches!(
            assignment.status.as_str(),
            models::project_task_board::assignment_status::AVAILABLE
                | models::project_task_board::assignment_status::CLAIMED
                | models::project_task_board::assignment_status::IN_PROGRESS
        ) {
            board_item_status = Some("in_progress");
            board_item_assigned_thread_id = Some(Some(assignment.thread_id));
            board_item_completed_at = Some(None);
        } else if matches!(
            assignment.status.as_str(),
            models::project_task_board::assignment_status::COMPLETED
                | models::project_task_board::assignment_status::REJECTED
                | models::project_task_board::assignment_status::CANCELLED
        ) {
            board_item_assigned_thread_id = Some(None);
            if !board_item_already_terminal {
                board_item_status = Some("pending");
                board_item_completed_at = Some(None);
            }
        } else if assignment.status == models::project_task_board::assignment_status::BLOCKED {
            let coordinator_thread_id = sqlx::query(
                r#"
                SELECT p.coordinator_thread_id
                FROM project_task_board_items i
                INNER JOIN project_task_boards b
                    ON b.id = i.board_id
                INNER JOIN actor_projects p
                    ON p.id = b.project_id
                WHERE i.id = $1
                  AND i.archived_at IS NULL
                  AND b.archived_at IS NULL
                  AND p.archived_at IS NULL
                "#,
            )
            .bind(assignment.board_item_id)
            .fetch_optional(&mut *tx)
            .await?
            .and_then(|row| row.try_get("coordinator_thread_id").ok());

            board_item_assigned_thread_id = Some(coordinator_thread_id);
            if !board_item_already_terminal {
                board_item_status = Some("pending");
                board_item_completed_at = Some(None);
            }
        }

        if board_item_status.is_some()
            || board_item_assigned_thread_id.is_some()
            || board_item_completed_at.is_some()
        {
            sqlx::query!(
                r#"
                UPDATE project_task_board_items
                SET
                    status = COALESCE($2, status),
                    assigned_thread_id = CASE
                        WHEN $3 THEN $4
                        ELSE assigned_thread_id
                    END,
                    completed_at = CASE
                        WHEN $5 THEN $6
                        ELSE completed_at
                    END,
                    updated_at = $7
                WHERE id = $1
                "#,
                assignment.board_item_id,
                board_item_status,
                board_item_assigned_thread_id.is_some(),
                board_item_assigned_thread_id.flatten(),
                board_item_completed_at.is_some(),
                board_item_completed_at.flatten(),
                now,
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(assignment)
    }
}

pub struct CancelBoardItemCommand {
    pub item_id: i64,
}

impl CancelBoardItemCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE project_task_board_items
            SET status = 'cancelled',
                completed_at = NOW(),
                pending_question = NULL,
                pending_approval = NULL,
                updated_at = NOW()
            WHERE id = $1
            "#,
            self.item_id,
        )
        .execute(executor)
        .await?;
        Ok(())
    }
}

pub struct CancelAssignmentsForBoardItemCommand {
    pub item_id: i64,
}

impl CancelAssignmentsForBoardItemCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE project_task_board_item_assignments
            SET status = 'cancelled',
                result_status = 'task_cancelled',
                result_summary = 'Task cancelled by user.',
                completed_at = NOW(),
                updated_at = NOW()
            WHERE board_item_id = $1
              AND status IN ('pending', 'available', 'blocked', 'claimed', 'in_progress')
            "#,
            self.item_id,
        )
        .execute(executor)
        .await?;
        Ok(())
    }
}

pub struct ClearBoardItemPendingFlagsCommand {
    pub item_id: i64,
}

impl ClearBoardItemPendingFlagsCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE project_task_board_items
            SET pending_question = NULL,
                pending_approval = NULL,
                updated_at = NOW()
            WHERE id = $1
            "#,
            self.item_id,
        )
        .execute(executor)
        .await?;
        Ok(())
    }
}

pub struct CreateBoardItemCommentCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub board_item_id: i64,
    pub actor_id: i64,
    pub body: String,
    pub metadata: serde_json::Value,
}

impl CreateBoardItemCommentCommand {
    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<models::ProjectTaskBoardItemComment, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let comment = sqlx::query_as!(
            models::ProjectTaskBoardItemComment,
            r#"
            INSERT INTO project_task_board_item_comments (
                id, deployment_id, board_item_id, actor_id, body, metadata,
                created_at, updated_at, archived_at, resolved_at, resolved_by_thread_id, resolution_summary
            ) VALUES (
                $1, $2, $3, $4, $5, $6::jsonb, NOW(), NOW(), NULL, NULL, NULL, NULL
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
            self.id,
            self.deployment_id,
            self.board_item_id,
            self.actor_id,
            self.body,
            self.metadata,
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::from)?;
        Ok(comment)
    }
}
