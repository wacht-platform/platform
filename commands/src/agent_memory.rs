use crate::Command;
use common::error::AppError;
use models::MemoryRecord;
use common::state::AppState;
use chrono::Utc;
use pgvector::HalfVector;

/// Command to create a new memory record
pub struct CreateMemoryCommand {
    pub id: i64,
    pub content: String,
    pub embedding: Vec<f32>,
    pub memory_category: String, // "procedural", "semantic", "episodic"
    pub creation_context_id: Option<i64>,
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
                creation_context_id, last_reinforced_at,
                semantic_centrality, uniqueness_score,
                compression_level, compressed_content,
                context_decay_profile,
                created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4,
                $5, 0, $6, $6,
                $7, $6,
                0.0, 0.0,
                0, NULL,
                '{}',
                $6, $6
            )
            RETURNING *
            "#,
        )
        .bind(self.id)
        .bind(self.content)
        .bind(embedding)
        .bind(self.memory_category)
        .bind(self.initial_importance)
        .bind(now)
        .bind(self.creation_context_id)
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
