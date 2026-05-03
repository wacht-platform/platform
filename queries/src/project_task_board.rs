use chrono::{DateTime, Utc};
use common::error::AppError;
use models::{
    ProjectTaskBoard, ProjectTaskBoardItem, ProjectTaskBoardItemAssignment,
    ProjectTaskBoardItemComment, ProjectTaskBoardItemRelation,
};

#[derive(Debug, Clone)]
pub struct PriorScheduleFire {
    pub task_key: String,
    pub status: String,
    pub fired_at: Option<DateTime<Utc>>,
}

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
                id, board_id, task_key, title, description, status,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at, state_version,
                schedule_id, scheduled_for, fired_at, pending_question, pending_approval
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
                id, board_id, task_key, title, description, status,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at, state_version,
                schedule_id, scheduled_for, fired_at, pending_question, pending_approval
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
                id, board_item_id, thread_id, assignment_role, status,
                instructions, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at, state_version
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
                id, board_id, task_key, title, description, status,
                assigned_thread_id, metadata, completed_at, archived_at, created_at, updated_at, state_version,
                schedule_id, scheduled_for, fired_at, pending_question, pending_approval
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
                id, board_item_id, thread_id, assignment_role, status,
                instructions, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at, state_version
            FROM project_task_board_item_assignments
            WHERE board_item_id = $1
            ORDER BY created_at ASC, id ASC
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
                id, board_item_id, thread_id, assignment_role, status,
                instructions, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at, state_version
            FROM project_task_board_item_assignments
            WHERE thread_id = $1
            ORDER BY status ASC, created_at ASC, id ASC
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
                id, board_item_id, thread_id, assignment_role, status,
                instructions, metadata, result_status, result_summary,
                result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
                updated_at, state_version
            FROM project_task_board_item_assignments
            WHERE board_item_id = $1
              AND status = $2
            ORDER BY created_at ASC, id ASC
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

pub struct GetParentTaskKeyQuery {
    pub board_item_id: i64,
}

impl GetParentTaskKeyQuery {
    pub fn new(board_item_id: i64) -> Self {
        Self { board_item_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<String>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT i.task_key AS "task_key!"
            FROM project_task_board_item_relations r
            JOIN project_task_board_items i ON r.parent_board_item_id = i.id
            WHERE r.child_board_item_id = $1
              AND r.relation_type = $2
            ORDER BY r.created_at ASC, r.id ASC
            LIMIT 1
            "#,
            self.board_item_id,
            models::project_task_board::relation_type::CHILD_OF
        )
        .fetch_optional(executor)
        .await?;
        Ok(row.map(|r| r.task_key))
    }
}

pub struct ListProjectTaskBoardItemCommentsQuery {
    pub board_item_id: i64,
}

impl ListProjectTaskBoardItemCommentsQuery {
    pub fn new(board_item_id: i64) -> Self {
        Self { board_item_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ProjectTaskBoardItemComment>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let comments = sqlx::query_as!(
            ProjectTaskBoardItemComment,
            r#"
            WITH target AS (
                SELECT id, schedule_id
                FROM project_task_board_items
                WHERE id = $1
            )
            SELECT
                c.id AS "id!",
                c.deployment_id AS "deployment_id!",
                c.board_item_id AS "board_item_id!",
                c.actor_id AS "actor_id!",
                c.body AS "body!",
                c.metadata AS "metadata!",
                c.created_at AS "created_at!",
                c.updated_at AS "updated_at!",
                c.archived_at,
                c.resolved_at,
                c.resolved_by_thread_id,
                c.resolution_summary
            FROM project_task_board_item_comments c
            INNER JOIN project_task_board_items i ON i.id = c.board_item_id
            INNER JOIN target t ON (
                i.id = t.id
                OR (t.schedule_id IS NOT NULL AND i.schedule_id = t.schedule_id)
            )
            WHERE c.archived_at IS NULL
              AND i.archived_at IS NULL
            ORDER BY c.created_at ASC, c.id ASC
            "#,
            self.board_item_id,
        )
        .fetch_all(executor)
        .await?;

        Ok(comments)
    }
}

pub struct ListPriorScheduleFiresQuery {
    pub board_item_id: i64,
    pub limit: i64,
}

impl ListPriorScheduleFiresQuery {
    pub fn new(board_item_id: i64, limit: i64) -> Self {
        Self {
            board_item_id,
            limit,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<PriorScheduleFire>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            r#"
            SELECT task_key AS "task_key!", status AS "status!", fired_at
            FROM project_task_board_items
            WHERE schedule_id = (
                SELECT schedule_id
                FROM project_task_board_items
                WHERE id = $1
            )
              AND id != $1
              AND archived_at IS NULL
              AND schedule_id IS NOT NULL
            ORDER BY fired_at DESC NULLS LAST, created_at DESC
            LIMIT $2
            "#,
            self.board_item_id,
            self.limit,
        )
        .fetch_all(executor)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| PriorScheduleFire {
                task_key: r.task_key,
                status: r.status,
                fired_at: r.fired_at,
            })
            .collect())
    }
}
