use common::error::AppError;
use models::execution_task_graph::status;
use models::{ExecutionTaskGraph, ExecutionTaskNode};

pub struct EnsureExecutionTaskGraphCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub context_id: i64,
}

impl EnsureExecutionTaskGraphCommand {
    pub fn new(id: i64, deployment_id: i64, context_id: i64) -> Self {
        Self {
            id,
            deployment_id,
            context_id,
        }
    }

    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<ExecutionTaskGraph, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = acquirer.begin().await?;

        let existing = sqlx::query!(
            r#"
            SELECT id, deployment_id, context_id, status, created_at, updated_at
            FROM execution_task_graphs
            WHERE deployment_id = $1 AND context_id = $2
            FOR UPDATE
            "#,
            self.deployment_id,
            self.context_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(row) = existing {
            if matches!(
                row.status.as_str(),
                status::GRAPH_COMPLETED | status::GRAPH_FAILED | status::GRAPH_CANCELLED
            ) {
                sqlx::query!(
                    r#"
                    DELETE FROM execution_task_graphs
                    WHERE id = $1
                    "#,
                    row.id
                )
                .execute(&mut *tx)
                .await?;
            } else {
                let active = sqlx::query!(
                    r#"
                    UPDATE execution_task_graphs
                    SET updated_at = NOW()
                    WHERE id = $1
                    RETURNING id, deployment_id, context_id, status, created_at, updated_at
                    "#,
                    row.id
                )
                .fetch_one(&mut *tx)
                .await?;

                tx.commit().await?;

                return Ok(ExecutionTaskGraph {
                    id: active.id,
                    deployment_id: active.deployment_id,
                    context_id: active.context_id,
                    status: active.status,
                    created_at: active.created_at,
                    updated_at: active.updated_at,
                });
            }
        }

        let created = sqlx::query!(
            r#"
            INSERT INTO execution_task_graphs (
                id, deployment_id, context_id, status, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, NOW(), NOW())
            RETURNING id, deployment_id, context_id, status, created_at, updated_at
            "#,
            self.id,
            self.deployment_id,
            self.context_id,
            status::GRAPH_ACTIVE
        )
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(ExecutionTaskGraph {
            id: created.id,
            deployment_id: created.deployment_id,
            context_id: created.context_id,
            status: created.status,
            created_at: created.created_at,
            updated_at: created.updated_at,
        })
    }
}

pub struct CreateExecutionTaskNodeCommand {
    pub id: i64,
    pub graph_id: i64,
    pub title: String,
    pub description: Option<String>,
    pub max_retries: i32,
    pub input: Option<serde_json::Value>,
}

impl CreateExecutionTaskNodeCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ExecutionTaskNode, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
    {
        let row = sqlx::query!(
            r#"
            INSERT INTO execution_task_nodes (
                id, graph_id, title, description, status, retry_count, max_retries,
                input, output, error, completed_at, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, 0, $6, $7, NULL, NULL, NULL, NOW(), NOW()
            )
            RETURNING
                id, graph_id, title, description, status, retry_count, max_retries,
                input, output, error, completed_at, created_at, updated_at
            "#,
            self.id,
            self.graph_id,
            self.title,
            self.description,
            status::NODE_PENDING,
            self.max_retries.max(0),
            self.input
        )
        .fetch_one(executor)
        .await?;

        sqlx::query!(
            r#"
            UPDATE execution_task_graphs
            SET status = $2, updated_at = NOW()
            WHERE id = $1 AND status != $2
            "#,
            self.graph_id,
            status::GRAPH_ACTIVE
        )
        .execute(executor)
        .await?;

        Ok(ExecutionTaskNode {
            id: row.id,
            graph_id: row.graph_id,
            title: row.title,
            description: row.description,
            status: row.status,
            retry_count: row.retry_count,
            max_retries: row.max_retries,
            input: row.input,
            output: row.output,
            error: row.error,
            completed_at: row.completed_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

pub struct AddExecutionTaskDependencyCommand {
    pub graph_id: i64,
    pub from_node_id: i64,
    pub to_node_id: i64,
}

impl AddExecutionTaskDependencyCommand {
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
            FROM execution_task_nodes
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
                FROM execution_task_edges e
                WHERE e.graph_id = $1 AND e.from_node_id = $2
                UNION
                SELECT e.to_node_id
                FROM execution_task_edges e
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
            INSERT INTO execution_task_edges (graph_id, from_node_id, to_node_id, created_at)
            VALUES ($1, $2, $3, NOW())
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
            UPDATE execution_task_graphs
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

pub struct CompleteExecutionTaskNodeCommand {
    pub graph_id: i64,
    pub node_id: i64,
    pub output: Option<serde_json::Value>,
}

impl CompleteExecutionTaskNodeCommand {
    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Option<ExecutionTaskNode>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
    {
        let row = sqlx::query!(
            r#"
            UPDATE execution_task_nodes
            SET
                status = $1,
                output = COALESCE($2, output),
                error = NULL,
                completed_at = NOW(),
                updated_at = NOW()
            WHERE graph_id = $3 AND id = $4
            RETURNING
                id, graph_id, title, description, status, retry_count, max_retries,
                input, output, error, completed_at, created_at, updated_at
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
                UPDATE execution_task_graphs
                SET updated_at = NOW()
                WHERE id = $1
                "#,
                self.graph_id
            )
            .execute(executor)
            .await?;
        }

        Ok(row.map(|r| ExecutionTaskNode {
            id: r.id,
            graph_id: r.graph_id,
            title: r.title,
            description: r.description,
            status: r.status,
            retry_count: r.retry_count,
            max_retries: r.max_retries,
            input: r.input,
            output: r.output,
            error: r.error,
            completed_at: r.completed_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }
}

pub struct FailExecutionTaskNodeCommand {
    pub graph_id: i64,
    pub node_id: i64,
    pub error: Option<serde_json::Value>,
}

impl FailExecutionTaskNodeCommand {
    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Option<ExecutionTaskNode>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
    {
        let row = sqlx::query!(
            r#"
            UPDATE execution_task_nodes
            SET
                retry_count = retry_count + 1,
                error = COALESCE($1, error, '{}'::jsonb),
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
                id, graph_id, title, description, status, retry_count, max_retries,
                input, output, error, completed_at, created_at, updated_at
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
                UPDATE execution_task_graphs
                SET updated_at = NOW()
                WHERE id = $1
                "#,
                self.graph_id
            )
            .execute(executor)
            .await?;
        }

        Ok(row.map(|r| ExecutionTaskNode {
            id: r.id,
            graph_id: r.graph_id,
            title: r.title,
            description: r.description,
            status: r.status,
            retry_count: r.retry_count,
            max_retries: r.max_retries,
            input: r.input,
            output: r.output,
            error: r.error,
            completed_at: r.completed_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }
}

pub struct MarkExecutionTaskNodeInProgressCommand {
    pub graph_id: i64,
    pub node_id: i64,
}

impl MarkExecutionTaskNodeInProgressCommand {
    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Option<ExecutionTaskNode>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
    {
        let row = sqlx::query!(
            r#"
            UPDATE execution_task_nodes
            SET
                status = $1,
                completed_at = NULL,
                updated_at = NOW()
            WHERE graph_id = $2 AND id = $3
            RETURNING
                id, graph_id, title, description, status, retry_count, max_retries,
                input, output, error, completed_at, created_at, updated_at
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
                UPDATE execution_task_graphs
                SET updated_at = NOW()
                WHERE id = $1
                "#,
                self.graph_id
            )
            .execute(executor)
            .await?;
        }

        Ok(row.map(|r| ExecutionTaskNode {
            id: r.id,
            graph_id: r.graph_id,
            title: r.title,
            description: r.description,
            status: r.status,
            retry_count: r.retry_count,
            max_retries: r.max_retries,
            input: r.input,
            output: r.output,
            error: r.error,
            completed_at: r.completed_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }
}

pub struct MarkExecutionTaskGraphCompletedCommand {
    pub graph_id: i64,
}

impl MarkExecutionTaskGraphCompletedCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ExecutionTaskGraph, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
    {
        let blocking = sqlx::query!(
            r#"
            SELECT COUNT(*)::bigint AS "count!"
            FROM execution_task_nodes
            WHERE graph_id = $1 AND status IN ($2, $3, $4)
            "#,
            self.graph_id,
            status::NODE_PENDING,
            status::NODE_IN_PROGRESS,
            status::NODE_FAILED
        )
        .fetch_one(executor)
        .await?;

        if blocking.count > 0 {
            return Err(AppError::BadRequest(
                "Cannot mark task graph completed while nodes are still pending, in progress, or failed".to_string(),
            ));
        }

        let row = sqlx::query!(
            r#"
            UPDATE execution_task_graphs
            SET status = $2, updated_at = NOW()
            WHERE id = $1
            RETURNING id, deployment_id, context_id, status, created_at, updated_at
            "#,
            self.graph_id,
            status::GRAPH_COMPLETED
        )
        .fetch_one(executor)
        .await?;

        Ok(ExecutionTaskGraph {
            id: row.id,
            deployment_id: row.deployment_id,
            context_id: row.context_id,
            status: row.status,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}
