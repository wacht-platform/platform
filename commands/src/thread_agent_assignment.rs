use common::error::AppError;
use models::ThreadAgentAssignment;
use sqlx::Row;

pub struct UpsertThreadAgentAssignmentCommand {
    pub thread_id: i64,
    pub agent_id: i64,
}

impl UpsertThreadAgentAssignmentCommand {
    pub fn new(thread_id: i64, agent_id: i64) -> Self {
        Self {
            thread_id,
            agent_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<ThreadAgentAssignment, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            INSERT INTO thread_agent_assignments (thread_id, agent_id, created_at, updated_at)
            SELECT t.id, a.id, NOW(), NOW()
            FROM agent_threads t
            JOIN ai_agents a
              ON a.id = $2
             AND a.deployment_id = t.deployment_id
            WHERE t.id = $1
            ON CONFLICT (thread_id)
            DO UPDATE SET
                agent_id = EXCLUDED.agent_id,
                updated_at = NOW()
            RETURNING thread_id, agent_id, created_at, updated_at
            "#,
        )
        .bind(self.thread_id)
        .bind(self.agent_id)
        .fetch_optional(executor)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| {
            AppError::BadRequest("Thread or agent not found, or deployment mismatch".to_string())
        })?;

        Ok(ThreadAgentAssignment {
            thread_id: row.get("thread_id"),
            agent_id: row.get("agent_id"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }
}
