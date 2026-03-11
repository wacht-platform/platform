use common::error::AppError;
use models::execution_task_graph::status;
use models::{ExecutionTaskEdge, ExecutionTaskGraph, ExecutionTaskGraphSummary, ExecutionTaskNode};

pub struct GetExecutionTaskGraphByContextQuery {
    pub deployment_id: i64,
    pub context_id: i64,
}

impl GetExecutionTaskGraphByContextQuery {
    pub fn new(deployment_id: i64, context_id: i64) -> Self {
        Self {
            deployment_id,
            context_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ExecutionTaskGraph>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query_as!(
            ExecutionTaskGraph,
            r#"
            SELECT id, deployment_id, context_id, status, created_at, updated_at
            FROM execution_task_graphs
            WHERE deployment_id = $1 AND context_id = $2
            "#,
            self.deployment_id,
            self.context_id
        )
        .fetch_optional(executor)
        .await?;

        Ok(row)
    }
}

pub struct GetExecutionTaskGraphByIdQuery {
    pub graph_id: i64,
}

impl GetExecutionTaskGraphByIdQuery {
    pub fn new(graph_id: i64) -> Self {
        Self { graph_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ExecutionTaskGraph>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query_as!(
            ExecutionTaskGraph,
            r#"
            SELECT id, deployment_id, context_id, status, created_at, updated_at
            FROM execution_task_graphs
            WHERE id = $1
            "#,
            self.graph_id
        )
        .fetch_optional(executor)
        .await?;

        Ok(row)
    }
}

pub struct ListExecutionTaskNodesQuery {
    pub graph_id: i64,
    pub include_terminal: bool,
}

impl ListExecutionTaskNodesQuery {
    pub fn new(graph_id: i64) -> Self {
        Self {
            graph_id,
            include_terminal: true,
        }
    }

    pub fn without_terminal(mut self) -> Self {
        self.include_terminal = false;
        self
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ExecutionTaskNode>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = if self.include_terminal {
            sqlx::query_as!(
                ExecutionTaskNode,
                r#"
                SELECT
                    id, graph_id, title, description, status, retry_count, max_retries,
                    input, output, error, completed_at, created_at, updated_at
                FROM execution_task_nodes
                WHERE graph_id = $1
                ORDER BY created_at ASC
                "#,
                self.graph_id
            )
            .fetch_all(executor)
            .await
        } else {
            sqlx::query_as!(
                ExecutionTaskNode,
                r#"
                SELECT
                    id, graph_id, title, description, status, retry_count, max_retries,
                    input, output, error, completed_at, created_at, updated_at
                FROM execution_task_nodes
                WHERE graph_id = $1 AND status NOT IN ($2, $3, $4)
                ORDER BY created_at ASC
                "#,
                self.graph_id,
                status::NODE_COMPLETED,
                status::NODE_FAILED,
                status::NODE_CANCELLED
            )
            .fetch_all(executor)
            .await
        }?;

        Ok(rows)
    }
}

pub struct ListReadyExecutionTaskNodesQuery {
    pub graph_id: i64,
}

impl ListReadyExecutionTaskNodesQuery {
    pub fn new(graph_id: i64) -> Self {
        Self { graph_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ExecutionTaskNode>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query_as!(
            ExecutionTaskNode,
            r#"
            SELECT
                n.id, n.graph_id, n.title, n.description, n.status, n.retry_count, n.max_retries,
                n.input, n.output, n.error, n.completed_at, n.created_at, n.updated_at
            FROM execution_task_nodes n
            WHERE
                n.graph_id = $1
                AND n.status = $2
                AND NOT EXISTS (
                    SELECT 1
                    FROM execution_task_edges e
                    JOIN execution_task_nodes dep
                      ON dep.id = e.from_node_id AND dep.graph_id = e.graph_id
                    WHERE
                        e.graph_id = n.graph_id
                        AND e.to_node_id = n.id
                        AND dep.status != $3
                )
            ORDER BY n.created_at ASC
            "#,
            self.graph_id,
            status::NODE_PENDING,
            status::NODE_COMPLETED
        )
        .fetch_all(executor)
        .await?;

        Ok(rows)
    }
}

pub struct GetExecutionTaskNodeByIdQuery {
    pub graph_id: i64,
    pub node_id: i64,
}

impl GetExecutionTaskNodeByIdQuery {
    pub fn new(graph_id: i64, node_id: i64) -> Self {
        Self { graph_id, node_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ExecutionTaskNode>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query_as!(
            ExecutionTaskNode,
            r#"
            SELECT
                id, graph_id, title, description, status, retry_count, max_retries,
                input, output, error, completed_at, created_at, updated_at
            FROM execution_task_nodes
            WHERE graph_id = $1 AND id = $2
            "#,
            self.graph_id,
            self.node_id
        )
        .fetch_optional(executor)
        .await?;

        Ok(row)
    }
}

pub struct ListExecutionTaskEdgesQuery {
    pub graph_id: i64,
}

impl ListExecutionTaskEdgesQuery {
    pub fn new(graph_id: i64) -> Self {
        Self { graph_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ExecutionTaskEdge>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query_as!(
            ExecutionTaskEdge,
            r#"
            SELECT graph_id, from_node_id, to_node_id, created_at
            FROM execution_task_edges
            WHERE graph_id = $1
            ORDER BY created_at ASC
            "#,
            self.graph_id
        )
        .fetch_all(executor)
        .await?;

        Ok(rows)
    }
}

pub struct GetExecutionTaskGraphSummaryQuery {
    pub graph_id: i64,
}

impl GetExecutionTaskGraphSummaryQuery {
    pub fn new(graph_id: i64) -> Self {
        Self { graph_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ExecutionTaskGraphSummary>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            WITH ready_nodes AS (
                SELECT n.id
                FROM execution_task_nodes n
                WHERE
                    n.graph_id = $1
                    AND n.status = $2
                    AND NOT EXISTS (
                        SELECT 1
                        FROM execution_task_edges e
                        JOIN execution_task_nodes dep
                          ON dep.id = e.from_node_id AND dep.graph_id = e.graph_id
                        WHERE
                            e.graph_id = n.graph_id
                            AND e.to_node_id = n.id
                            AND dep.status != $3
                    )
            )
            SELECT
                g.id,
                g.status,
                COUNT(n.id)::bigint AS "total_nodes!",
                COUNT(*) FILTER (WHERE n.status = $2)::bigint AS "pending_nodes!",
                COUNT(*) FILTER (WHERE n.id IN (SELECT id FROM ready_nodes))::bigint AS "ready_nodes!",
                COUNT(*) FILTER (WHERE n.status = $4)::bigint AS "in_progress_nodes!",
                COUNT(*) FILTER (WHERE n.status = $3)::bigint AS "completed_nodes!",
                COUNT(*) FILTER (WHERE n.status = $5)::bigint AS "failed_nodes!",
                COUNT(*) FILTER (WHERE n.status = $6)::bigint AS "cancelled_nodes!"
            FROM execution_task_graphs g
            LEFT JOIN execution_task_nodes n ON n.graph_id = g.id
            WHERE g.id = $1
            GROUP BY g.id, g.status
            "#,
            self.graph_id,
            status::NODE_PENDING,
            status::NODE_COMPLETED,
            status::NODE_IN_PROGRESS,
            status::NODE_FAILED,
            status::NODE_CANCELLED
        )
        .fetch_optional(executor)
        .await?;

        let Some(r) = row else {
            return Ok(None);
        };

        let progress_percent = if r.total_nodes == 0 {
            0.0
        } else {
            ((r.completed_nodes as f64) / (r.total_nodes as f64) * 100.0).min(100.0)
        };

        Ok(Some(ExecutionTaskGraphSummary {
            graph_id: r.id,
            graph_status: r.status,
            total_nodes: r.total_nodes,
            pending_nodes: r.pending_nodes,
            ready_nodes: r.ready_nodes,
            in_progress_nodes: r.in_progress_nodes,
            completed_nodes: r.completed_nodes,
            failed_nodes: r.failed_nodes,
            cancelled_nodes: r.cancelled_nodes,
            progress_percent,
        }))
    }
}
