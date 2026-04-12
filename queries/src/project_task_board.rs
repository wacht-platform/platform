use common::error::AppError;
use models::{
    ProjectTaskBoard, ProjectTaskBoardItem, ProjectTaskBoardItemAssignment,
    ProjectTaskBoardItemEvent, ProjectTaskBoardItemRelation,
};

pub struct GetProjectTaskBoardByProjectIdQuery {
    pub project_id: i64,
    pub deployment_id: i64,
}

impl GetProjectTaskBoardByProjectIdQuery {
    pub fn new(project_id: i64, deployment_id: i64) -> Self {
        Self {
            project_id,
            deployment_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ProjectTaskBoard>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let board = sqlx::query_as::<_, ProjectTaskBoard>(
            r#"
            SELECT
                id, deployment_id, actor_id, project_id, title, status, metadata,
                created_at, updated_at, archived_at
            FROM project_task_boards
            WHERE deployment_id = $1 AND project_id = $2 AND archived_at IS NULL
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.project_id)
        .fetch_optional(executor)
        .await?;

        Ok(board)
    }
}

pub struct GetProjectTaskBoardByIdQuery {
    pub board_id: i64,
    pub deployment_id: i64,
}

impl GetProjectTaskBoardByIdQuery {
    pub fn new(board_id: i64, deployment_id: i64) -> Self {
        Self {
            board_id,
            deployment_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ProjectTaskBoard>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let board = sqlx::query_as::<_, ProjectTaskBoard>(
            r#"
            SELECT
                id, deployment_id, actor_id, project_id, title, status, metadata,
                created_at, updated_at, archived_at
            FROM project_task_boards
            WHERE id = $1 AND deployment_id = $2 AND archived_at IS NULL
            LIMIT 1
            "#,
        )
        .bind(self.board_id)
        .bind(self.deployment_id)
        .fetch_optional(executor)
        .await?;

        Ok(board)
    }
}

pub struct ListProjectTaskBoardItemsQuery {
    pub board_id: i64,
}

impl ListProjectTaskBoardItemsQuery {
    pub fn new(board_id: i64) -> Self {
        Self { board_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ProjectTaskBoardItem>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let items = sqlx::query_as::<_, ProjectTaskBoardItem>(
            r#"
            SELECT
                id, board_id, task_key, title, description, status, priority,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at
            FROM project_task_board_items
            WHERE board_id = $1 AND archived_at IS NULL
            ORDER BY created_at ASC
            "#,
        )
        .bind(self.board_id)
        .fetch_all(executor)
        .await?;

        Ok(items)
    }
}

pub struct GetProjectTaskBoardItemByIdQuery {
    pub item_id: i64,
}

impl GetProjectTaskBoardItemByIdQuery {
    pub fn new(item_id: i64) -> Self {
        Self { item_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ProjectTaskBoardItem>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let item = sqlx::query_as::<_, ProjectTaskBoardItem>(
            r#"
            SELECT
                id, board_id, task_key, title, description, status, priority,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at
            FROM project_task_board_items
            WHERE id = $1 AND archived_at IS NULL
            "#,
        )
        .bind(self.item_id)
        .fetch_optional(executor)
        .await?;

        Ok(item)
    }
}

pub struct GetProjectTaskBoardItemAssignmentByIdQuery {
    pub assignment_id: i64,
}

impl GetProjectTaskBoardItemAssignmentByIdQuery {
    pub fn new(assignment_id: i64) -> Self {
        Self { assignment_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ProjectTaskBoardItemAssignment>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let assignment = sqlx::query_as::<_, ProjectTaskBoardItemAssignment>(
            r#"
            SELECT
                id, board_item_id, thread_id, assignment_role, assignment_order, status,
                instructions, handoff_file_path, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at
            FROM project_task_board_item_assignments
            WHERE id = $1
            LIMIT 1
            "#,
        )
        .bind(self.assignment_id)
        .fetch_optional(executor)
        .await?;

        Ok(assignment)
    }
}

pub struct GetProjectTaskBoardItemByTaskKeyQuery {
    pub board_id: i64,
    pub task_key: String,
}

impl GetProjectTaskBoardItemByTaskKeyQuery {
    pub fn new(board_id: i64, task_key: impl Into<String>) -> Self {
        Self {
            board_id,
            task_key: task_key.into(),
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ProjectTaskBoardItem>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let item = sqlx::query_as::<_, ProjectTaskBoardItem>(
            r#"
            SELECT
                id, board_id, task_key, title, description, status, priority,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at
            FROM project_task_board_items
            WHERE board_id = $1 AND task_key = $2 AND archived_at IS NULL
            LIMIT 1
            "#,
        )
        .bind(self.board_id)
        .bind(&self.task_key)
        .fetch_optional(executor)
        .await?;

        Ok(item)
    }
}

pub struct ListProjectTaskBoardItemEventsQuery {
    pub board_item_id: i64,
}

impl ListProjectTaskBoardItemEventsQuery {
    pub fn new(board_item_id: i64) -> Self {
        Self { board_item_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ProjectTaskBoardItemEvent>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let events = sqlx::query_as::<_, ProjectTaskBoardItemEvent>(
            r#"
            SELECT
                id, board_item_id, thread_id, execution_run_id, event_type, summary,
                body_markdown, details, created_at
            FROM project_task_board_item_events
            WHERE board_item_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(self.board_item_id)
        .fetch_all(executor)
        .await?;

        Ok(events)
    }
}

pub struct ListProjectTaskBoardItemAssignmentsQuery {
    pub board_item_id: i64,
}

impl ListProjectTaskBoardItemAssignmentsQuery {
    pub fn new(board_item_id: i64) -> Self {
        Self { board_item_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ProjectTaskBoardItemAssignment>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let assignments = sqlx::query_as::<_, ProjectTaskBoardItemAssignment>(
            r#"
            SELECT
                id, board_item_id, thread_id, assignment_role, assignment_order, status,
                instructions, handoff_file_path, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at
            FROM project_task_board_item_assignments
            WHERE board_item_id = $1
            ORDER BY assignment_order ASC, created_at ASC
            "#,
        )
        .bind(self.board_item_id)
        .fetch_all(executor)
        .await?;

        Ok(assignments)
    }
}

pub struct ListAssignmentsForThreadQuery {
    pub thread_id: i64,
}

impl ListAssignmentsForThreadQuery {
    pub fn new(thread_id: i64) -> Self {
        Self { thread_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ProjectTaskBoardItemAssignment>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let assignments = sqlx::query_as::<_, ProjectTaskBoardItemAssignment>(
            r#"
            SELECT
                id, board_item_id, thread_id, assignment_role, assignment_order, status,
                instructions, handoff_file_path, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at
            FROM project_task_board_item_assignments
            WHERE thread_id = $1
            ORDER BY status ASC, assignment_order ASC, created_at ASC
            "#,
        )
        .bind(self.thread_id)
        .fetch_all(executor)
        .await?;

        Ok(assignments)
    }
}

pub struct GetNextAvailableAssignmentForBoardItemQuery {
    pub board_item_id: i64,
}

impl GetNextAvailableAssignmentForBoardItemQuery {
    pub fn new(board_item_id: i64) -> Self {
        Self { board_item_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ProjectTaskBoardItemAssignment>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let assignment = sqlx::query_as::<_, ProjectTaskBoardItemAssignment>(
            r#"
            SELECT
                id, board_item_id, thread_id, assignment_role, assignment_order, status,
                instructions, handoff_file_path, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at
            FROM project_task_board_item_assignments
            WHERE board_item_id = $1
              AND status = $2
            ORDER BY assignment_order ASC, created_at ASC
            LIMIT 1
            "#,
        )
        .bind(self.board_item_id)
        .bind(models::project_task_board::assignment_status::AVAILABLE)
        .fetch_optional(executor)
        .await?;

        Ok(assignment)
    }
}

pub struct ListProjectTaskBoardRelationsQuery {
    pub board_id: i64,
}

impl ListProjectTaskBoardRelationsQuery {
    pub fn new(board_id: i64) -> Self {
        Self { board_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ProjectTaskBoardItemRelation>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let relations = sqlx::query_as::<_, ProjectTaskBoardItemRelation>(
            r#"
            SELECT
                id,
                board_id,
                parent_board_item_id,
                child_board_item_id,
                relation_type,
                metadata,
                created_at
            FROM project_task_board_item_relations
            WHERE board_id = $1
            ORDER BY created_at ASC, id ASC
            "#,
        )
        .bind(self.board_id)
        .fetch_all(executor)
        .await?;

        Ok(relations)
    }
}

pub struct ListProjectTaskBoardItemRelationsQuery {
    pub board_item_id: i64,
}

impl ListProjectTaskBoardItemRelationsQuery {
    pub fn new(board_item_id: i64) -> Self {
        Self { board_item_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ProjectTaskBoardItemRelation>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let relations = sqlx::query_as::<_, ProjectTaskBoardItemRelation>(
            r#"
            SELECT
                id,
                board_id,
                parent_board_item_id,
                child_board_item_id,
                relation_type,
                metadata,
                created_at
            FROM project_task_board_item_relations
            WHERE parent_board_item_id = $1 OR child_board_item_id = $1
            ORDER BY created_at ASC, id ASC
            "#,
        )
        .bind(self.board_item_id)
        .fetch_all(executor)
        .await?;

        Ok(relations)
    }
}
