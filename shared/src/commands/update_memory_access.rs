use crate::error::AppError;
use crate::state::AppState;
use crate::commands::Command;
use chrono::Utc;

pub struct UpdateMemoryAccessCommand {
    memory_ids: Vec<i64>,
}

impl UpdateMemoryAccessCommand {
    pub fn new(memory_ids: Vec<i64>) -> Self {
        Self { memory_ids }
    }
}

impl Command for UpdateMemoryAccessCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        if self.memory_ids.is_empty() {
            return Ok(());
        }

        let query = r#"
            UPDATE agent_execution_memories 
            SET access_count = access_count + 1,
                last_accessed_at = $1
            WHERE id = ANY($2)
        "#;

        sqlx::query(query)
            .bind(Utc::now())
            .bind(&self.memory_ids)
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

        Ok(())
    }
}

pub struct AdjustMemoryImportanceCommand {
    memory_id: i64,
    importance_delta: f32,
}

impl AdjustMemoryImportanceCommand {
    pub fn new(memory_id: i64, importance_delta: f32) -> Self {
        Self { memory_id, importance_delta }
    }
}

impl Command for AdjustMemoryImportanceCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let query = r#"
            UPDATE agent_execution_memories 
            SET importance = LEAST(1.0, GREATEST(0.1, importance + $1))
            WHERE id = $2
        "#;

        sqlx::query(query)
            .bind(self.importance_delta)
            .bind(self.memory_id)
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

        Ok(())
    }
}