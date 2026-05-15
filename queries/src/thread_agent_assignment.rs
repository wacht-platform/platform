use common::error::AppError;
use models::ThreadAgentAssignment;
use sqlx::Row;

pub struct GetThreadAgentAssignmentQuery {
    pub thread_id: i64,
}

impl GetThreadAgentAssignmentQuery {
    pub fn new(thread_id: i64) -> Self {
        Self { thread_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ThreadAgentAssignment>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            SELECT thread_id, agent_id, created_at, updated_at
            FROM thread_agent_assignments
            WHERE thread_id = $1
            "#,
        )
        .bind(self.thread_id)
        .fetch_optional(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(row.map(|row| ThreadAgentAssignment {
            thread_id: row.get("thread_id"),
            agent_id: row.get("agent_id"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }))
    }
}

pub struct ListThreadAgentAssignmentsForThreadsQuery {
    pub thread_ids: Vec<i64>,
}

pub struct ThreadAgentAssignmentWithName {
    pub thread_id: i64,
    pub agent_id: i64,
    pub agent_name: String,
}

impl ListThreadAgentAssignmentsForThreadsQuery {
    pub fn new(thread_ids: Vec<i64>) -> Self {
        Self { thread_ids }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ThreadAgentAssignmentWithName>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if self.thread_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            r#"
            SELECT taa.thread_id AS thread_id,
                   taa.agent_id AS agent_id,
                   a.name AS agent_name
            FROM thread_agent_assignments taa
            JOIN ai_agents a ON a.id = taa.agent_id
            WHERE taa.thread_id = ANY($1::bigint[])
            "#,
        )
        .bind(&self.thread_ids)
        .fetch_all(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(rows
            .into_iter()
            .map(|row| ThreadAgentAssignmentWithName {
                thread_id: row.get("thread_id"),
                agent_id: row.get("agent_id"),
                agent_name: row.get("agent_name"),
            })
            .collect())
    }
}

pub struct ResolveThreadExecutionAgentQuery {
    pub thread_id: i64,
    pub deployment_id: i64,
}

impl ResolveThreadExecutionAgentQuery {
    pub fn new(thread_id: i64, deployment_id: i64) -> Self {
        Self {
            thread_id,
            deployment_id,
        }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            SELECT taa.agent_id AS agent_id
            FROM agent_threads t
            LEFT JOIN thread_agent_assignments taa
                ON taa.thread_id = t.id
            WHERE t.id = $1 AND t.deployment_id = $2
            "#,
        )
        .bind(self.thread_id)
        .bind(self.deployment_id)
        .fetch_optional(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(row.and_then(|row| row.try_get("agent_id").ok()))
    }
}
