use common::error::AppError;
use models::thread_task_graph::status;
use models::{ThreadTaskEdge, ThreadTaskGraph, ThreadTaskGraphSummary, ThreadTaskNode};

pub struct GetLatestThreadTaskGraphQuery {
    pub deployment_id: i64,
    pub thread_id: i64,
}

impl GetLatestThreadTaskGraphQuery {
    pub fn new(deployment_id: i64, thread_id: i64) -> Self {
        Self {
            deployment_id,
            thread_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ThreadTaskGraph>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query_as!(
            ThreadTaskGraph,
            r#"
            SELECT id, deployment_id, thread_id, board_item_id, version, status, metadata, created_at, updated_at
            FROM thread_task_graphs
            WHERE deployment_id = $1 AND thread_id = $2
            ORDER BY version DESC
            LIMIT 1
            "#,
            self.deployment_id,
            self.thread_id
        )
        .fetch_optional(executor)
        .await?;

        Ok(row)
    }
}

pub struct GetThreadTaskGraphByIdQuery {
    pub graph_id: i64,
}

impl GetThreadTaskGraphByIdQuery {
    pub fn new(graph_id: i64) -> Self {
        Self { graph_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ThreadTaskGraph>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query_as!(
            ThreadTaskGraph,
            r#"
            SELECT id, deployment_id, thread_id, board_item_id, version, status, metadata, created_at, updated_at
            FROM thread_task_graphs
            WHERE id = $1
            "#,
            self.graph_id
        )
        .fetch_optional(executor)
        .await?;

        Ok(row)
    }
}

pub struct ListThreadTaskNodesQuery {
    pub graph_id: i64,
    pub include_terminal: bool,
}

impl ListThreadTaskNodesQuery {
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<ThreadTaskNode>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = if self.include_terminal {
            sqlx::query_as!(
                ThreadTaskNode,
                r#"
                SELECT
                    id, graph_id, board_item_id, title, description, status, priority,
                    owner_agent_id, assigned_thread_id, retry_count, max_retries,
                    input, output, error, lease_owner, lease_until, completed_at, created_at, updated_at
                FROM thread_task_nodes
                WHERE graph_id = $1
                ORDER BY created_at ASC
                "#,
                self.graph_id
            )
            .fetch_all(executor)
            .await
        } else {
            sqlx::query_as!(
                ThreadTaskNode,
                r#"
                SELECT
                    id, graph_id, board_item_id, title, description, status, priority,
                    owner_agent_id, assigned_thread_id, retry_count, max_retries,
                    input, output, error, lease_owner, lease_until, completed_at, created_at, updated_at
                FROM thread_task_nodes
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

pub struct ListReadyThreadTaskNodesQuery {
    pub graph_id: i64,
}

impl ListReadyThreadTaskNodesQuery {
    pub fn new(graph_id: i64) -> Self {
        Self { graph_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<ThreadTaskNode>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query_as!(
            ThreadTaskNode,
            r#"
            SELECT
                n.id, n.graph_id, n.board_item_id, n.title, n.description, n.status, n.priority,
                n.owner_agent_id, n.assigned_thread_id, n.retry_count, n.max_retries,
                n.input, n.output, n.error, n.lease_owner, n.lease_until, n.completed_at, n.created_at, n.updated_at
            FROM thread_task_nodes n
            WHERE
                n.graph_id = $1
                AND n.status = $2
                AND NOT EXISTS (
                    SELECT 1
                    FROM thread_task_edges e
                    JOIN thread_task_nodes dep
                      ON dep.id = e.from_node_id AND dep.graph_id = e.graph_id
                    WHERE
                        e.graph_id = n.graph_id
                        AND e.to_node_id = n.id
                        AND dep.status != $3
                )
            ORDER BY n.priority ASC, n.created_at ASC
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

pub struct GetThreadTaskNodeByIdQuery {
    pub graph_id: i64,
    pub node_id: i64,
}

impl GetThreadTaskNodeByIdQuery {
    pub fn new(graph_id: i64, node_id: i64) -> Self {
        Self { graph_id, node_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ThreadTaskNode>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query_as!(
            ThreadTaskNode,
            r#"
            SELECT
                id, graph_id, board_item_id, title, description, status, priority,
                owner_agent_id, assigned_thread_id, retry_count, max_retries,
                input, output, error, lease_owner, lease_until, completed_at, created_at, updated_at
            FROM thread_task_nodes
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

pub struct ListThreadTaskEdgesQuery {
    pub graph_id: i64,
}

impl ListThreadTaskEdgesQuery {
    pub fn new(graph_id: i64) -> Self {
        Self { graph_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<ThreadTaskEdge>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query_as!(
            ThreadTaskEdge,
            r#"
            SELECT graph_id, from_node_id, to_node_id, dependency_type, created_at
            FROM thread_task_edges
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

pub struct GetThreadTaskGraphSummaryQuery {
    pub graph_id: i64,
}

impl GetThreadTaskGraphSummaryQuery {
    pub fn new(graph_id: i64) -> Self {
        Self { graph_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ThreadTaskGraphSummary>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            WITH ready_nodes AS (
                SELECT n.id
                FROM thread_task_nodes n
                WHERE
                    n.graph_id = $1
                    AND n.status = $2
                    AND NOT EXISTS (
                        SELECT 1
                        FROM thread_task_edges e
                        JOIN thread_task_nodes dep
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
            FROM thread_task_graphs g
            LEFT JOIN thread_task_nodes n ON n.graph_id = g.id
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

        Ok(Some(ThreadTaskGraphSummary {
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
