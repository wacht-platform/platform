use crate::{DispatchThreadEventCommand, EnqueueThreadEventCommand};
use chrono::Utc;
use common::{
    HasDbRouter, HasIdProvider, HasNatsJetStreamProvider, ReadConsistency, error::AppError,
};
use models::{
    ProjectTaskBoard, ProjectTaskBoardItem, ProjectTaskBoardItemAssignment,
    ProjectTaskBoardItemAssignmentEventDetails, ProjectTaskBoardItemEvent,
    ProjectTaskBoardItemRelation,
};
use queries::{GetProjectTaskBoardItemByIdQuery, ListProjectTaskBoardItemAssignmentsQuery};
use sqlx::Row;

fn assignment_details(
    assignment: &ProjectTaskBoardItemAssignment,
    note: Option<String>,
) -> serde_json::Value {
    serde_json::to_value(ProjectTaskBoardItemAssignmentEventDetails {
        assignment_id: assignment.id,
        board_item_id: assignment.board_item_id,
        thread_id: assignment.thread_id,
        assignment_role: assignment.assignment_role.clone(),
        assignment_order: assignment.assignment_order,
        status: assignment.status.clone(),
        result_status: assignment.result_status.clone(),
        result_summary: assignment.result_summary.clone(),
        result_payload: assignment.result_payload.clone(),
        note,
        instructions: assignment.instructions.clone(),
        handoff_file_path: assignment.handoff_file_path.clone(),
        metadata: assignment.typed_metadata(),
    })
    .unwrap_or_else(|_| serde_json::Value::Null)
}

fn assignment_thread_event_payload(
    assignment: &ProjectTaskBoardItemAssignment,
) -> serde_json::Value {
    serde_json::json!({
        "assignment_id": assignment.id.to_string(),
    })
}

fn board_item_status_is_terminal(status: &str) -> bool {
    matches!(status, "completed" | "cancelled")
}

async fn create_assignment_board_item_event_with_deps<D>(
    deps: &D,
    assignment: &ProjectTaskBoardItemAssignment,
    event_type: &str,
    summary: &str,
    note: Option<String>,
) -> Result<ProjectTaskBoardItemEvent, AppError>
where
    D: HasDbRouter + HasIdProvider + ?Sized,
{
    CreateProjectTaskBoardItemEventCommand {
        id: deps.id_provider().next_id()? as i64,
        board_item_id: assignment.board_item_id,
        thread_id: Some(assignment.thread_id),
        execution_run_id: None,
        event_type: event_type.to_string(),
        summary: summary.to_string(),
        body_markdown: None,
        details: assignment_details(assignment, note),
    }
    .execute_with_db(deps.writer_pool())
    .await
}

async fn enqueue_assignment_execution_event_with_deps<D>(
    deps: &D,
    assignment: &ProjectTaskBoardItemAssignment,
) -> Result<(), AppError>
where
    D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + ?Sized,
{
    let thread = sqlx::query!(
        r#"
        SELECT deployment_id, project_id, actor_id, status
        FROM agent_threads
        WHERE id = $1 AND archived_at IS NULL
        "#,
        assignment.thread_id
    )
    .fetch_optional(deps.reader_pool(ReadConsistency::Strong))
    .await?;

    let Some(thread) = thread else {
        return Ok(());
    };

    DispatchThreadEventCommand::new(
        EnqueueThreadEventCommand::new(
            deps.id_provider().next_id()? as i64,
            thread.deployment_id,
            assignment.thread_id,
            models::thread_event::event_type::ASSIGNMENT_EXECUTION.to_string(),
        )
        .with_board_item_id(assignment.board_item_id)
        .with_priority(20)
        .with_payload(assignment_thread_event_payload(assignment)),
    )
    .execute_with_deps(deps)
    .await?;

    Ok(())
}

async fn enqueue_board_item_to_coordinator_with_deps<D>(
    deps: &D,
    board_item: &ProjectTaskBoardItem,
    note: Option<String>,
    caused_by_thread_id: Option<i64>,
) -> Result<(), AppError>
where
    D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + ?Sized,
{
    let coordinator = sqlx::query!(
        r#"
        SELECT
            p.coordinator_thread_id,
            t.deployment_id,
            t.project_id,
            t.status
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

    CreateProjectTaskBoardItemEventCommand {
        id: deps.id_provider().next_id()? as i64,
        board_item_id: board_item.id,
        thread_id: Some(coordinator_thread_id),
        execution_run_id: None,
        event_type: "task_returned_to_coordinator".to_string(),
        summary: "Task returned to coordinator for rerouting".to_string(),
        body_markdown: None,
        details: serde_json::json!({
            "board_item_id": board_item.id.to_string(),
            "task_key": board_item.task_key,
            "status": board_item.status,
            "note": note,
        }),
    }
    .execute_with_db(deps.writer_pool())
    .await?;

    let payload = models::thread_event::TaskRoutingEventPayload {
        board_item_id: board_item.id,
    };

    let mut enqueue = EnqueueThreadEventCommand::new(
        deps.id_provider().next_id()? as i64,
        coordinator.deployment_id,
        coordinator_thread_id,
        models::thread_event::event_type::TASK_ROUTING.to_string(),
    )
    .with_board_item_id(board_item.id)
    .with_priority(15)
    .with_payload(serde_json::to_value(payload).map_err(|err| {
        AppError::Internal(format!(
            "Failed to serialize coordinator routing payload: {}",
            err
        ))
    })?);

    if let Some(caused_by_thread_id) = caused_by_thread_id {
        enqueue = enqueue.with_caused_by_thread_id(caused_by_thread_id);
    }

    DispatchThreadEventCommand::new(enqueue)
        .execute_with_deps(deps)
        .await?;

    Ok(())
}

async fn preempt_board_item_work_with_deps<D>(
    deps: &D,
    board_item: &ProjectTaskBoardItem,
    note: String,
) -> Result<(), AppError>
where
    D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + ?Sized,
{
    let now = Utc::now();

    sqlx::query(
        r#"
        UPDATE thread_events
        SET status = 'cancelled', updated_at = $2
        WHERE board_item_id = $1
          AND status = 'pending'
          AND event_type IN (
            'task_routing',
            'assignment_execution',
            'assignment_outcome_review'
          )
        "#,
    )
    .bind(board_item.id)
    .bind(now)
    .execute(deps.writer_pool())
    .await?;

    sqlx::query(
        r#"
        UPDATE project_task_board_item_assignments
        SET
            status = 'cancelled',
            result_status = COALESCE(result_status, 'cancelled'),
            result_summary = COALESCE(result_summary, $2),
            updated_at = $3
        WHERE board_item_id = $1
          AND status IN ('pending', 'available', 'claimed', 'in_progress', 'blocked')
        "#,
    )
    .bind(board_item.id)
    .bind(note.clone())
    .bind(now)
    .execute(deps.writer_pool())
    .await?;

    if let Some(thread_id) = board_item.assigned_thread_id {
        let thread = sqlx::query(
            r#"
            SELECT deployment_id
            FROM agent_threads
            WHERE id = $1 AND archived_at IS NULL
            "#,
        )
        .bind(thread_id)
        .fetch_optional(deps.reader_pool(ReadConsistency::Strong))
        .await?;

        if let Some(thread) = thread {
            let deployment_id: i64 = thread.get("deployment_id");
            let _ = DispatchThreadEventCommand::new(
                EnqueueThreadEventCommand::new(
                    deps.id_provider().next_id()? as i64,
                    deployment_id,
                    thread_id,
                    models::thread_event::event_type::CONTROL_INTERRUPT.to_string(),
                )
                .with_payload(serde_json::json!({
                    "board_item_id": board_item.id.to_string(),
                    "task_key": board_item.task_key.clone(),
                    "note": note.clone(),
                })),
            )
            .execute_with_deps(deps)
            .await?;
        }
    }

    sqlx::query(
        r#"
        UPDATE project_task_board_items
        SET assigned_thread_id = NULL, updated_at = $2
        WHERE id = $1
        "#,
    )
    .bind(board_item.id)
    .bind(now)
    .execute(deps.writer_pool())
    .await?;

    Ok(())
}

async fn maybe_ready_parent_after_child_completion_with_deps<D>(
    deps: &D,
    child_item: &ProjectTaskBoardItem,
) -> Result<Option<ProjectTaskBoardItem>, AppError>
where
    D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + ?Sized,
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

    let child_task_keys = child_rows
        .iter()
        .map(|row| row.get::<String, _>("task_key"))
        .collect::<Vec<_>>();
    let now = Utc::now();
    let parent_item = sqlx::query_as::<_, ProjectTaskBoardItem>(
        r#"
        UPDATE project_task_board_items
        SET status = 'pending', updated_at = $2
        WHERE id = $1
        RETURNING
            id, board_id, task_key, title, description, status, priority,
            assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at
        "#,
    )
    .bind(parent_item.id)
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;

    CreateProjectTaskBoardItemEventCommand {
        id: deps.id_provider().next_id()? as i64,
        board_item_id: parent_item.id,
        thread_id: child_item.assigned_thread_id,
        execution_run_id: None,
        event_type: "child_tasks_completed".to_string(),
        summary: "All child tasks completed".to_string(),
        body_markdown: None,
        details: serde_json::json!({
            "task_key": parent_item.task_key,
            "child_task_keys": child_task_keys,
        }),
    }
    .execute_with_db(&mut *tx)
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

pub struct CreateProjectTaskBoardItemCommand {
    pub id: i64,
    pub board_id: i64,
    pub task_key: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    pub assigned_thread_id: Option<i64>,
    pub metadata: serde_json::Value,
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
                id, board_id, task_key, title, description, status, priority,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8, $9,
                CASE
                    WHEN $6::text = 'completed' THEN $10::timestamptz
                    ELSE NULL::timestamptz
                END,
                NULL,
                $10, $10
            )
            RETURNING
                id, board_id, task_key, title, description, status, priority,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at
            "#,
        )
        .bind(self.id)
        .bind(self.board_id)
        .bind(self.task_key)
        .bind(self.title)
        .bind(self.description)
        .bind(self.status)
        .bind(self.priority)
        .bind(self.assigned_thread_id)
        .bind(self.metadata)
        .bind(now)
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
                id, board_id, task_key, title, description, status, priority,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at
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
                id, board_id, task_key, title, description, status, priority,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at
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
    pub board_id: i64,
    pub task_key: String,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub metadata: serde_json::Value,
}

impl UpdateProjectTaskBoardItemCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<ProjectTaskBoardItem, AppError>
    where
        D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + ?Sized,
    {
        let mut tx = deps.writer_pool().begin().await?;

        let requested_status = self.status.clone();
        let requested_priority = self.priority.clone();
        let requested_metadata = self.metadata.clone();

        let item = self.execute_with_db(&mut *tx).await?;

        let (event_type, summary) = match requested_status.as_deref() {
            Some("completed") => ("task_completed", "Task completed"),
            Some("blocked") => ("task_blocked", "Task blocked"),
            Some("failed") => ("task_failed", "Task failed"),
            Some("in_progress") => ("task_in_progress", "Task moved in progress"),
            _ => ("task_updated", "Task updated"),
        };

        let mut details = serde_json::Map::new();
        details.insert("task_key".to_string(), serde_json::json!(item.task_key));
        if let Some(ref status) = requested_status {
            details.insert("status".to_string(), serde_json::json!(status));
        }
        if let Some(ref priority) = requested_priority {
            details.insert("priority".to_string(), serde_json::json!(priority));
        }
        details.insert("metadata".to_string(), requested_metadata);

        CreateProjectTaskBoardItemEventCommand {
            id: deps.id_provider().next_id()? as i64,
            board_item_id: item.id,
            thread_id: item.assigned_thread_id,
            execution_run_id: None,
            event_type: event_type.to_string(),
            summary: summary.to_string(),
            body_markdown: None,
            details: serde_json::Value::Object(details),
        }
        .execute_with_db(&mut *tx)
        .await?;

        tx.commit().await?;

        let should_preempt = item.assigned_thread_id.is_some()
            && !board_item_status_is_terminal(&item.status)
            && requested_status.as_deref() != Some("completed");
        if should_preempt {
            preempt_board_item_work_with_deps(
                deps,
                &item,
                "Task updated while active work existed; interrupted current routing and returned control to coordinator"
                    .to_string(),
            )
            .await?;
        }

        let _ = maybe_ready_parent_after_child_completion_with_deps(deps, &item).await?;
        ReconcileProjectTaskBoardItemCommand::new(item.id)
            .with_note("Task changed; scheduler reevaluated routing".to_string())
            .execute_with_deps(deps)
            .await?;

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
                priority = COALESCE($4, priority),
                metadata = $5,
                completed_at = CASE
                    WHEN COALESCE($3, status) = 'completed' THEN COALESCE(completed_at, $6)
                    WHEN $3 IS NOT NULL THEN NULL
                    ELSE completed_at
                END,
                updated_at = $6
            WHERE board_id = $1 AND task_key = $2 AND archived_at IS NULL
            RETURNING
                id, board_id, task_key, title, description, status, priority,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at
            "#,
        )
        .bind(self.board_id)
        .bind(&task_key)
        .bind(self.status)
        .bind(self.priority)
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

pub struct CreateProjectTaskBoardItemEventCommand {
    pub id: i64,
    pub board_item_id: i64,
    pub thread_id: Option<i64>,
    pub execution_run_id: Option<i64>,
    pub event_type: String,
    pub summary: String,
    pub body_markdown: Option<String>,
    pub details: serde_json::Value,
}

pub struct ReconcileProjectTaskBoardItemCommand {
    pub board_item_id: i64,
    pub note: Option<String>,
    pub caused_by_thread_id: Option<i64>,
}

impl CreateProjectTaskBoardItemEventCommand {
    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<ProjectTaskBoardItemEvent, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let event = sqlx::query_as!(
            ProjectTaskBoardItemEvent,
            r#"
            INSERT INTO project_task_board_item_events (
                id, board_item_id, thread_id, execution_run_id, event_type, summary, body_markdown, details, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
            RETURNING id, board_item_id, thread_id, execution_run_id, event_type, summary, body_markdown, details, created_at
            "#,
            self.id,
            self.board_item_id,
            self.thread_id,
            self.execution_run_id,
            self.event_type,
            self.summary,
            self.body_markdown,
            self.details,
        )
        .fetch_one(executor)
        .await?;

        Ok(event)
    }
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
        D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + ?Sized,
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

        if assignments
            .iter()
            .any(|assignment| matches!(assignment.status.as_str(), "claimed" | "in_progress"))
        {
            return Ok(());
        }

        if let Some(assignment) = assignments
            .iter()
            .filter(|assignment| {
                assignment.status == models::project_task_board::assignment_status::AVAILABLE
            })
            .min_by_key(|assignment| assignment.assignment_order)
        {
            enqueue_assignment_execution_event_with_deps(deps, assignment).await?;
            return Ok(());
        }

        if let Some(next_assignment) = assignments
            .iter()
            .filter(|assignment| {
                assignment.status == models::project_task_board::assignment_status::PENDING
            })
            .min_by_key(|assignment| assignment.assignment_order)
        {
            let activation_note = self.note.clone().unwrap_or_else(|| {
                format!(
                    "Scheduler activated assignment order {}",
                    next_assignment.assignment_order
                )
            });

            let activated = UpdateProjectTaskBoardItemAssignmentStateCommand::new(
                next_assignment.id,
                models::project_task_board::assignment_status::AVAILABLE.to_string(),
            )
            .with_note(activation_note.clone())
            .without_reconcile()
            .execute_with_db(deps.writer_pool())
            .await?;

            create_assignment_board_item_event_with_deps(
                deps,
                &activated,
                "assignment_available",
                "Task assignment is now available",
                Some(activation_note),
            )
            .await?;

            enqueue_assignment_execution_event_with_deps(deps, &activated).await?;
            return Ok(());
        }

        enqueue_board_item_to_coordinator_with_deps(
            deps,
            &board_item,
            self.note.or_else(|| {
                Some("No active assignment available; returned task to coordinator".to_string())
            }),
            self.caused_by_thread_id,
        )
        .await?;

        Ok(())
    }
}

pub struct CreateProjectTaskBoardItemAssignmentCommand {
    pub id: i64,
    pub board_item_id: i64,
    pub thread_id: i64,
    pub assignment_role: String,
    pub assignment_order: i32,
    pub status: String,
    pub instructions: Option<String>,
    pub handoff_file_path: Option<String>,
    pub metadata: serde_json::Value,
}

impl CreateProjectTaskBoardItemAssignmentCommand {
    pub async fn execute_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<ProjectTaskBoardItemAssignment, AppError>
    where
        D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + ?Sized,
    {
        let assignment = self.execute_with_db(deps.writer_pool()).await?;

        create_assignment_board_item_event_with_deps(
            deps,
            &assignment,
            "assignment_created",
            "Task assignment created",
            None,
        )
        .await?;

        if assignment.status == models::project_task_board::assignment_status::AVAILABLE {
            create_assignment_board_item_event_with_deps(
                deps,
                &assignment,
                "assignment_available",
                "Task assignment is now available",
                None,
            )
            .await?;
        }

        ReconcileProjectTaskBoardItemCommand::new(assignment.board_item_id)
            .with_note("Task assignment created; scheduler reevaluated routing".to_string())
            .execute_with_deps(deps)
            .await?;

        Ok(assignment)
    }

    pub async fn execute_with_db<'a, A>(
        self,
        acquirer: A,
    ) -> Result<ProjectTaskBoardItemAssignment, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let now = Utc::now();
        let mut tx = acquirer.begin().await?;
        let assignment = sqlx::query_as!(
            ProjectTaskBoardItemAssignment,
            r#"
            INSERT INTO project_task_board_item_assignments (
                id, board_item_id, thread_id, assignment_role, assignment_order, status,
                instructions, handoff_file_path, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6,
                $7, $8, $9, NULL, NULL,
                NULL, NULL, NULL, NULL, NULL, $10, $10
            )
            RETURNING
                id, board_item_id, thread_id, assignment_role, assignment_order, status,
                instructions, handoff_file_path, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at
            "#,
            self.id,
            self.board_item_id,
            self.thread_id,
            self.assignment_role,
            self.assignment_order,
            self.status,
            self.instructions,
            self.handoff_file_path,
            self.metadata,
            now,
        )
        .fetch_one(&mut *tx)
        .await?;

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
        Ok(assignment)
    }
}

pub struct UpdateProjectTaskBoardItemAssignmentCommand {
    pub assignment_id: i64,
    pub thread_id: i64,
    pub assignment_role: String,
    pub status: String,
    pub instructions: Option<String>,
    pub handoff_file_path: Option<String>,
    pub metadata: serde_json::Value,
}

impl UpdateProjectTaskBoardItemAssignmentCommand {
    pub async fn execute_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<ProjectTaskBoardItemAssignment, AppError>
    where
        D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + ?Sized,
    {
        let assignment = self.execute_with_db(deps.writer_pool()).await?;

        create_assignment_board_item_event_with_deps(
            deps,
            &assignment,
            "assignment_updated",
            "Task assignment updated",
            None,
        )
        .await?;

        if assignment.status == models::project_task_board::assignment_status::AVAILABLE {
            create_assignment_board_item_event_with_deps(
                deps,
                &assignment,
                "assignment_available",
                "Task assignment is now available",
                None,
            )
            .await?;
        }

        ReconcileProjectTaskBoardItemCommand::new(assignment.board_item_id)
            .with_note("Task assignment updated; scheduler reevaluated routing".to_string())
            .execute_with_deps(deps)
            .await?;

        Ok(assignment)
    }

    pub async fn execute_with_db<'a, A>(
        self,
        acquirer: A,
    ) -> Result<ProjectTaskBoardItemAssignment, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = acquirer.begin().await?;
        let now = Utc::now();

        let current = sqlx::query_as!(
            ProjectTaskBoardItemAssignment,
            r#"
            SELECT
                id, board_item_id, thread_id, assignment_role, assignment_order, status,
                instructions, handoff_file_path, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at
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
                handoff_file_path = $6,
                metadata = $7,
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
                updated_at = $8
            WHERE id = $1
            RETURNING
                id, board_item_id, thread_id, assignment_role, assignment_order, status,
                instructions, handoff_file_path, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at
            "#,
            self.assignment_id,
            self.thread_id,
            self.assignment_role,
            self.status,
            self.instructions,
            self.handoff_file_path,
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
        D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + ?Sized,
    {
        let suppress_reconcile = self.suppress_reconcile;
        let note = self.note.clone();
        let assignment = self.execute_with_db(deps.writer_pool()).await?;

        let (event_type, summary) = match assignment.status.as_str() {
            models::project_task_board::assignment_status::AVAILABLE => {
                ("assignment_available", "Task assignment is now available")
            }
            models::project_task_board::assignment_status::CLAIMED => {
                ("assignment_claimed", "Task assignment was claimed")
            }
            models::project_task_board::assignment_status::IN_PROGRESS => (
                "assignment_in_progress",
                "Task assignment moved in progress",
            ),
            models::project_task_board::assignment_status::COMPLETED => {
                ("assignment_completed", "Task assignment completed")
            }
            models::project_task_board::assignment_status::REJECTED => {
                ("assignment_rejected", "Task assignment was rejected")
            }
            models::project_task_board::assignment_status::BLOCKED => {
                ("assignment_blocked", "Task assignment is blocked")
            }
            models::project_task_board::assignment_status::CANCELLED => {
                ("assignment_cancelled", "Task assignment was cancelled")
            }
            _ => ("assignment_updated", "Task assignment updated"),
        };

        create_assignment_board_item_event_with_deps(
            deps,
            &assignment,
            event_type,
            summary,
            note.clone(),
        )
        .await?;

        if !suppress_reconcile {
            let reconcile_note = note.unwrap_or_else(|| {
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

    pub async fn execute_with_db<'a, A>(
        self,
        acquirer: A,
    ) -> Result<ProjectTaskBoardItemAssignment, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = acquirer.begin().await?;
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
                id, board_item_id, thread_id, assignment_role, assignment_order, status,
                instructions, handoff_file_path, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at
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
