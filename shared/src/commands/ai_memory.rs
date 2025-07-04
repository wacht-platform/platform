use crate::{
    commands::Command,
    error::AppError,
    models::ai_memory::{MemoryRecord, MemoryType},
    state::AppState,
};
use chrono::Utc;
use pgvector::Vector;
use sqlx::Row;

pub struct CreateMemoryCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub agent_id: i64,
    pub execution_context_id: Option<i64>,
    pub memory_type: MemoryType,
    pub content: String,
    pub embedding: Vec<f32>,
    pub importance: f32,
}

impl Command for CreateMemoryCommand {
    type Output = MemoryRecord;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();
        let embedding = Vector::from(self.embedding);

        let row = sqlx::query(
            r#"
            INSERT INTO agent_execution_memories (id, deployment_id, agent_id, execution_context_id, memory_type, content, embedding, importance, access_count, created_at, last_accessed_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING id, deployment_id, agent_id, execution_context_id, memory_type, content, embedding, importance, access_count, created_at, last_accessed_at
            "#,
        )
        .bind(self.id)
        .bind(self.deployment_id)
        .bind(self.agent_id)
        .bind(self.execution_context_id)
        .bind(self.memory_type.as_str())
        .bind(self.content)
        .bind(embedding)
        .bind(self.importance)
        .bind(0i32) // access_count
        .bind(now) // created_at
        .bind(now) // last_accessed_at
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        let memory = MemoryRecord {
            id: row.try_get("id").map_err(AppError::from)?,
            deployment_id: row.try_get("deployment_id").map_err(AppError::from)?,
            agent_id: row.try_get("agent_id").map_err(AppError::from)?,
            execution_context_id: row
                .try_get("execution_context_id")
                .map_err(AppError::from)?,
            memory_type: row.try_get("memory_type").map_err(AppError::from)?,
            content: row.try_get("content").map_err(AppError::from)?,
            embedding: row.try_get("embedding").map_err(AppError::from)?,
            importance: row.try_get("importance").map_err(AppError::from)?,
            access_count: row.try_get("access_count").map_err(AppError::from)?,
            created_at: row.try_get("created_at").map_err(AppError::from)?,
            last_accessed_at: row.try_get("last_accessed_at").map_err(AppError::from)?,
        };

        Ok(memory)
    }
}

pub struct DeleteAgentMemoriesCommand {
    pub agent_id: i64,
}

impl Command for DeleteAgentMemoriesCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!("DELETE FROM agent_execution_memories WHERE agent_id = $1", self.agent_id)
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

        Ok(())
    }
}

pub struct DeleteExecutionContextMemoriesCommand {
    pub execution_context_id: i64,
}

impl Command for DeleteExecutionContextMemoriesCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!(
            "DELETE FROM agent_execution_memories WHERE execution_context_id = $1",
            self.execution_context_id
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(())
    }
}
