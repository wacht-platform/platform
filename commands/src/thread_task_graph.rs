use common::error::AppError;
use models::thread_task_graph::status;
use models::{ThreadTaskGraph, ThreadTaskNode};

pub struct EnsureThreadTaskGraphCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub thread_id: i64,
    pub board_item_id: Option<i64>,
}

impl EnsureThreadTaskGraphCommand {
    pub fn new(id: i64, deployment_id: i64, thread_id: i64) -> Self {
        Self {
            id,
            deployment_id,
            thread_id,
            board_item_id: None,
        }
    }

    pub fn with_board_item_id(mut self, board_item_id: i64) -> Self {
        self.board_item_id = Some(board_item_id);
        self
    }

    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<ThreadTaskGraph, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = acquirer.begin().await?;

        let existing = sqlx::query_as!(
            ThreadTaskGraph,
            r#"
            SELECT id, deployment_id, thread_id, board_item_id, status, metadata, created_at, updated_at
            FROM thread_task_graphs
            WHERE deployment_id = $1
              AND thread_id = $2
              AND (($3::bigint IS NULL AND board_item_id IS NULL) OR board_item_id = $3)
              AND status = $4
            ORDER BY id DESC
            LIMIT 1
            FOR UPDATE
            "#,
            self.deployment_id,
            self.thread_id,
            self.board_item_id,
            status::GRAPH_ACTIVE,
        )
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(active) = existing {
            let refreshed = sqlx::query_as!(
                ThreadTaskGraph,
                r#"
                UPDATE thread_task_graphs
                SET updated_at = NOW()
                WHERE id = $1
                RETURNING id, deployment_id, thread_id, board_item_id, status, metadata, created_at, updated_at
                "#,
                active.id
            )
            .fetch_one(&mut *tx)
            .await?;

            tx.commit().await?;
            return Ok(refreshed);
        }

        let created = sqlx::query_as!(
            ThreadTaskGraph,
            r#"
            INSERT INTO thread_task_graphs (
                id, deployment_id, thread_id, board_item_id, status, metadata, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, '{}'::jsonb, NOW(), NOW())
            RETURNING id, deployment_id, thread_id, board_item_id, status, metadata, created_at, updated_at
            "#,
            self.id,
            self.deployment_id,
            self.thread_id,
            self.board_item_id,
            status::GRAPH_ACTIVE,
        )
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(created)
    }
}

pub struct CreateThreadTaskNodeCommand {
    pub id: i64,
    pub graph_id: i64,
    pub board_item_id: Option<i64>,
    pub title: String,
    pub description: Option<String>,
    pub max_retries: i32,
    pub input: Option<serde_json::Value>,
}

impl CreateThreadTaskNodeCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ThreadTaskNode, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
    {
        let row = sqlx::query_as!(
            ThreadTaskNode,
            r#"
            INSERT INTO thread_task_nodes (
                id, graph_id, board_item_id, title, description, status, priority,
                owner_agent_id, assigned_thread_id, retry_count, max_retries,
                input, output, error, lease_owner, lease_until, completed_at, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, 100,
                NULL, NULL, 0, $7,
                $8, NULL, NULL, NULL, NULL, NULL, NOW(), NOW()
            )
            RETURNING
                id, graph_id, board_item_id, title, description, status, priority,
                owner_agent_id, assigned_thread_id, retry_count, max_retries,
                input, output, error, lease_owner, lease_until, completed_at, created_at, updated_at
            "#,
            self.id,
            self.graph_id,
            self.board_item_id,
            self.title,
            self.description,
            status::NODE_PENDING,
            self.max_retries.max(0),
            self.input,
        )
        .fetch_one(executor)
        .await?;

        sqlx::query!(
            r#"
            UPDATE thread_task_graphs
            SET status = $2, updated_at = NOW()
            WHERE id = $1 AND status != $2
            "#,
            self.graph_id,
            status::GRAPH_ACTIVE
        )
        .execute(executor)
        .await?;

        Ok(row)
    }
}

pub struct AddThreadTaskDependencyCommand {
    pub graph_id: i64,
    pub from_node_id: i64,
    pub to_node_id: i64,
}

impl AddThreadTaskDependencyCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
    {
        if self.from_node_id == self.to_node_id {
            return Err(AppError::BadRequest(
                "A task node cannot depend on itself".to_string(),
            ));
        }

        let exists_row = sqlx::query!(
            r#"
            SELECT COUNT(*)::bigint AS "count!"
            FROM thread_task_nodes
            WHERE graph_id = $1 AND (id = $2 OR id = $3)
            "#,
            self.graph_id,
            self.from_node_id,
            self.to_node_id
        )
        .fetch_one(executor)
        .await?;

        if exists_row.count != 2 {
            return Err(AppError::NotFound(
                "One or more task nodes were not found in graph".to_string(),
            ));
        }

        let cycle_row = sqlx::query!(
            r#"
            WITH RECURSIVE reachable AS (
                SELECT e.to_node_id
                FROM thread_task_edges e
                WHERE e.graph_id = $1 AND e.from_node_id = $2
                UNION
                SELECT e.to_node_id
                FROM thread_task_edges e
                INNER JOIN reachable r ON r.to_node_id = e.from_node_id
                WHERE e.graph_id = $1
            )
            SELECT EXISTS(
                SELECT 1 FROM reachable WHERE to_node_id = $3
            ) AS "creates_cycle!"
            "#,
            self.graph_id,
            self.to_node_id,
            self.from_node_id
        )
        .fetch_one(executor)
        .await?;

        if cycle_row.creates_cycle {
            return Err(AppError::BadRequest(
                "Dependency creates a cycle in task graph".to_string(),
            ));
        }

        sqlx::query!(
            r#"
            INSERT INTO thread_task_edges (graph_id, from_node_id, to_node_id, dependency_type, created_at)
            VALUES ($1, $2, $3, 'hard', NOW())
            ON CONFLICT (graph_id, from_node_id, to_node_id) DO NOTHING
            "#,
            self.graph_id,
            self.from_node_id,
            self.to_node_id
        )
        .execute(executor)
        .await?;

        sqlx::query!(
            r#"
            UPDATE thread_task_graphs
            SET status = $2, updated_at = NOW()
            WHERE id = $1 AND status != $2
            "#,
            self.graph_id,
            status::GRAPH_ACTIVE
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

pub struct CompleteThreadTaskNodeCommand {
    pub graph_id: i64,
    pub node_id: i64,
    pub output: Option<serde_json::Value>,
}

impl CompleteThreadTaskNodeCommand {
    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Option<ThreadTaskNode>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
    {
        let row = sqlx::query_as!(
            ThreadTaskNode,
            r#"
            UPDATE thread_task_nodes
            SET
                status = $1,
                output = COALESCE($2, output),
                error = NULL,
                lease_owner = NULL,
                lease_until = NULL,
                completed_at = NOW(),
                updated_at = NOW()
            WHERE graph_id = $3 AND id = $4
            RETURNING
                id, graph_id, board_item_id, title, description, status, priority,
                owner_agent_id, assigned_thread_id, retry_count, max_retries,
                input, output, error, lease_owner, lease_until, completed_at, created_at, updated_at
            "#,
            status::NODE_COMPLETED,
            self.output,
            self.graph_id,
            self.node_id
        )
        .fetch_optional(executor)
        .await?;

        if row.is_some() {
            sqlx::query!(
                r#"
                UPDATE thread_task_graphs
                SET updated_at = NOW()
                WHERE id = $1
                "#,
                self.graph_id
            )
            .execute(executor)
            .await?;
        }

        Ok(row)
    }
}

pub struct FailThreadTaskNodeCommand {
    pub graph_id: i64,
    pub node_id: i64,
    pub error: Option<serde_json::Value>,
}

impl FailThreadTaskNodeCommand {
    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Option<ThreadTaskNode>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
    {
        let row = sqlx::query_as!(
            ThreadTaskNode,
            r#"
            UPDATE thread_task_nodes
            SET
                retry_count = retry_count + 1,
                error = COALESCE($1, error, '{}'::jsonb),
                lease_owner = NULL,
                lease_until = NULL,
                status = CASE
                    WHEN retry_count + 1 <= max_retries THEN $2
                    ELSE $3
                END,
                completed_at = CASE
                    WHEN retry_count + 1 <= max_retries THEN NULL
                    ELSE NOW()
                END,
                updated_at = NOW()
            WHERE graph_id = $4 AND id = $5
            RETURNING
                id, graph_id, board_item_id, title, description, status, priority,
                owner_agent_id, assigned_thread_id, retry_count, max_retries,
                input, output, error, lease_owner, lease_until, completed_at, created_at, updated_at
            "#,
            self.error,
            status::NODE_PENDING,
            status::NODE_FAILED,
            self.graph_id,
            self.node_id
        )
        .fetch_optional(executor)
        .await?;

        if row.is_some() {
            sqlx::query!(
                r#"
                UPDATE thread_task_graphs
                SET updated_at = NOW()
                WHERE id = $1
                "#,
                self.graph_id
            )
            .execute(executor)
            .await?;
        }

        Ok(row)
    }
}

pub struct MarkThreadTaskNodeInProgressCommand {
    pub graph_id: i64,
    pub node_id: i64,
}

impl MarkThreadTaskNodeInProgressCommand {
    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Option<ThreadTaskNode>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
    {
        let row = sqlx::query_as!(
            ThreadTaskNode,
            r#"
            UPDATE thread_task_nodes
            SET
                status = $1,
                completed_at = NULL,
                updated_at = NOW()
            WHERE graph_id = $2 AND id = $3
            RETURNING
                id, graph_id, board_item_id, title, description, status, priority,
                owner_agent_id, assigned_thread_id, retry_count, max_retries,
                input, output, error, lease_owner, lease_until, completed_at, created_at, updated_at
            "#,
            status::NODE_IN_PROGRESS,
            self.graph_id,
            self.node_id,
        )
        .fetch_optional(executor)
        .await?;

        if row.is_some() {
            sqlx::query!(
                r#"
                UPDATE thread_task_graphs
                SET updated_at = NOW()
                WHERE id = $1
                "#,
                self.graph_id
            )
            .execute(executor)
            .await?;
        }

        Ok(row)
    }
}

pub struct CancelThreadTaskGraphCommand {
    pub graph_id: i64,
}

impl CancelThreadTaskGraphCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ThreadTaskGraph, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
    {
        sqlx::query(
            r#"
            UPDATE thread_task_nodes
            SET
                status = CASE
                    WHEN status IN ($2, $3) THEN $4
                    ELSE status
                END,
                lease_owner = NULL,
                lease_until = NULL,
                completed_at = CASE
                    WHEN status IN ($2, $3) AND completed_at IS NULL THEN NOW()
                    ELSE completed_at
                END,
                updated_at = NOW()
            WHERE graph_id = $1
            "#,
        )
        .bind(self.graph_id)
        .bind(status::NODE_PENDING)
        .bind(status::NODE_IN_PROGRESS)
        .bind(status::NODE_CANCELLED)
        .execute(executor)
        .await?;

        let row = sqlx::query_as::<_, ThreadTaskGraph>(
            r#"
            UPDATE thread_task_graphs
            SET status = $2, updated_at = NOW()
            WHERE id = $1
            RETURNING id, deployment_id, thread_id, board_item_id, status, metadata, created_at, updated_at
            "#,
        )
        .bind(self.graph_id)
        .bind(status::GRAPH_CANCELLED)
        .fetch_one(executor)
        .await?;

        Ok(row)
    }
}
