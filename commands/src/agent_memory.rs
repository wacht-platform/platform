use crate::Command;
use chrono::Utc;
use common::error::AppError;
use common::state::AppState;
use dto::json::agent_memory::MemoryCategory;
use models::MemoryRecord;
use pgvector::HalfVector;

/// Command to create a new memory record
pub struct CreateMemoryCommand {
    pub id: i64,
    pub content: String,
    pub embedding: Vec<f32>,
    pub memory_category: MemoryCategory,
    pub creation_context_id: Option<i64>,
    pub agent_id: Option<i64>,
    pub initial_importance: f64,
}

impl Command for CreateMemoryCommand {
    type Output = MemoryRecord;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();
        let embedding = if self.embedding.is_empty() {
            None
        } else {
            Some(HalfVector::from_f32_slice(&self.embedding))
        };

        let record = sqlx::query_as::<_, MemoryRecord>(
            r#"
            INSERT INTO memories (
                id, content, embedding, memory_category,
                base_temporal_score, access_count, first_accessed_at, last_accessed_at,
                creation_context_id, agent_id, last_reinforced_at,
                semantic_centrality, uniqueness_score,
                compression_level, compressed_content,
                created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4,
                $5, 0, $6, $6,
                $7, $8, $6,
                0.0, 0.0,
                0, NULL,
                $6, $6
            )
            RETURNING id, content, embedding, memory_category,
                base_temporal_score, access_count, first_accessed_at, last_accessed_at,
                creation_context_id, agent_id, last_reinforced_at,
                semantic_centrality, uniqueness_score,
                compression_level, compressed_content,
                created_at, updated_at
            "#,
        )
        .bind(self.id)
        .bind(self.content)
        .bind(embedding)
        .bind(self.memory_category.to_string())
        .bind(self.initial_importance)
        .bind(now)
        .bind(self.creation_context_id)
        .bind(self.agent_id)
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        Ok(record)
    }
}

/// Update memory access metrics
pub struct UpdateMemoryAccessCommand {
    pub memory_id: i64,
}

impl Command for UpdateMemoryAccessCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query(
            r#"
            UPDATE memories
            SET access_count = access_count + 1,
                last_accessed_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(self.memory_id)
        .execute(&app_state.db_pool)
        .await?;

        Ok(())
    }
}

/// Delete multiple memories (used for consolidation)
pub struct DeleteMemoriesCommand {
    pub memory_ids: Vec<i64>,
}

impl Command for DeleteMemoriesCommand {
    type Output = u64;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        if self.memory_ids.is_empty() {
            return Ok(0);
        }
        
        let result = sqlx::query(
            r#"DELETE FROM memories WHERE id = ANY($1)"#,
        )
        .bind(&self.memory_ids)
        .execute(&app_state.db_pool)
        .await?;

        Ok(result.rows_affected())
    }
}
