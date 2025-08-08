use crate::{
    commands::Command,
    error::AppError,
    models::MemoryBoundaries,
    state::AppState,
};
use chrono::Utc;
use serde_json::json;
use sqlx::Row;

/// Create default memory boundaries for a context
pub struct CreateMemoryBoundariesCommand {
    pub context_id: i64,
}

impl Command for CreateMemoryBoundariesCommand {
    type Output = MemoryBoundaries;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let default_memory_limits = json!({
            "procedural": 100,
            "semantic": 500,
            "episodic": 200,
            "working": 50
        });
        
        let now = Utc::now();
        
        let row = sqlx::query(
            r#"
            INSERT INTO memory_boundaries (
                context_id,
                max_conversations,
                max_memories_per_category,
                compression_threshold_days,
                eviction_threshold_score,
                created_at,
                updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING 
                context_id,
                max_conversations,
                max_memories_per_category,
                compression_threshold_days,
                eviction_threshold_score,
                created_at,
                updated_at
            "#
        )
        .bind(self.context_id)
        .bind(1000i32) // Default max conversations
        .bind(&default_memory_limits)
        .bind(30i32) // Compress after 30 days
        .bind(0.1f64) // Evict if score below 0.1
        .bind(now)
        .bind(now)
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;
        
        let boundaries = MemoryBoundaries {
            context_id: row.try_get("context_id")?,
            max_conversations: row.try_get("max_conversations")?,
            max_memories_per_category: row.try_get("max_memories_per_category")?,
            compression_threshold_days: row.try_get("compression_threshold_days")?,
            eviction_threshold_score: row.try_get("eviction_threshold_score")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        };
        
        Ok(boundaries)
    }
}


/// Evict low-score items
pub struct EvictLowScoreItemsCommand {
    pub context_id: i64,
    pub threshold: f64,
}

impl Command for EvictLowScoreItemsCommand {
    type Output = i64;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Only delete low-score memories, not conversations
        let mem_result = sqlx::query!(
            r#"
            DELETE FROM memories
            WHERE creation_context_id = $1
              AND base_temporal_score < $2
              AND created_at < NOW() - INTERVAL '7 days'
            "#,
            self.context_id,
            self.threshold as f64
        )
        .execute(&app_state.db_pool)
        .await?;
        
        Ok(mem_result.rows_affected() as i64)
    }
}

/// Enforce conversation limit
pub struct EnforceConversationLimitCommand {
    pub context_id: i64,
    pub max_conversations: i32,
}

impl Command for EnforceConversationLimitCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!(
            r#"
            DELETE FROM conversations
            WHERE id IN (
                SELECT id
                FROM conversations
                WHERE context_id = $1
                ORDER BY created_at ASC
                OFFSET $2
            )
            "#,
            self.context_id,
            self.max_conversations as i64
        )
        .execute(&app_state.db_pool)
        .await?;
        
        Ok(())
    }
}

/// Enforce memory limits per category
pub struct EnforceMemoryLimitsCommand {
    pub context_id: i64,
    pub limits: serde_json::Value,
}

impl Command for EnforceMemoryLimitsCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let limits = self.limits.as_object()
            .ok_or_else(|| AppError::Internal("Invalid memory limits format".to_string()))?;
        
        for (category, limit) in limits {
            let max_count = limit.as_i64().unwrap_or(100);
            
            // Delete excess memories in this category
            sqlx::query!(
                r#"
                DELETE FROM memories
                WHERE id IN (
                    SELECT id
                    FROM memories
                    WHERE creation_context_id = $1
                      AND memory_category = $2
                    ORDER BY base_temporal_score ASC, created_at ASC
                    OFFSET $3
                )
                "#,
                self.context_id,
                category,
                max_count
            )
            .execute(&app_state.db_pool)
            .await?;
        }
        
        Ok(())
    }
}

/// Check if cleanup is needed
pub struct CheckCleanupNeededQuery {
    pub context_id: i64,
}

impl crate::queries::Query for CheckCleanupNeededQuery {
    type Output = bool;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // First get or create boundaries
        let boundaries = match (crate::queries::GetMemoryBoundariesQuery {
            context_id: self.context_id,
        })
        .execute(app_state)
        .await
        {
            Ok(b) => b,
            Err(_) => {
                // Create default boundaries if not exist
                CreateMemoryBoundariesCommand {
                    context_id: self.context_id,
                }
                .execute(app_state)
                .await?
            }
        };
        
        // Check conversation count
        let conv_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) as "count!"
            FROM conversations
            WHERE context_id = $1
            "#,
            self.context_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;
        
        if conv_count > boundaries.max_conversations as i64 {
            return Ok(true);
        }
        
        // No longer need to check for compression since conversations don't have compression_level
        Ok(false)
    }
}

