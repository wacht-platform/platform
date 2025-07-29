use crate::{
    error::AppError,
    models::{ConversationRecord, MemoryBoundaries, MemoryRecord},
    queries::Query,
    state::AppState,
};
use chrono::{DateTime, Utc};
use pgvector::Vector;
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Debug)]
pub struct GetMRUMemoriesQuery {
    pub limit: i64,
}

impl Query for GetMRUMemoriesQuery {
    type Output = Vec<MemoryRecord>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let records = sqlx::query_as::<_, MemoryRecord>(
            r#"
            SELECT
                id, content, embedding, memory_category,
                base_temporal_score, access_count,
                first_accessed_at, last_accessed_at,
                citation_count, cross_context_value, learning_confidence,
                relevance_score, usefulness_score,
                creation_context_id, last_reinforced_at,
                semantic_centrality, uniqueness_score,
                compression_level, compressed_content,
                context_decay_profile,
                created_at, updated_at
            FROM memories
            ORDER BY last_accessed_at DESC
            LIMIT $1
            "#,
        )
        .bind(self.limit)
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        Ok(records)
    }
}

#[derive(Debug)]
pub struct GetRecentConversationsQuery {
    pub context_id: i64,
    pub limit: i64,
}

impl GetRecentConversationsQuery {
    pub fn new(context_id: i64, limit: i64) -> Self {
        Self { context_id, limit }
    }
}

impl Query for GetRecentConversationsQuery {
    type Output = Vec<ConversationRecord>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let records = sqlx::query_as::<_, ConversationRecord>(
            r#"
            SELECT
                id, context_id, timestamp, content, message_type,
                created_at, updated_at
            FROM conversations
            WHERE context_id = $1
                AND message_type != 'execution_summary'
            ORDER BY id DESC
            LIMIT $2
            "#,
        )
        .bind(self.context_id)
        .bind(self.limit)
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        Ok(records)
    }
}

#[derive(Debug)]
pub struct GetLLMConversationHistoryQuery {
    pub context_id: i64,
    pub limit: i64,
}

impl GetLLMConversationHistoryQuery {
    pub fn new(context_id: i64, limit: i64) -> Self {
        Self { context_id, limit }
    }
}

impl Query for GetLLMConversationHistoryQuery {
    type Output = Vec<ConversationRecord>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let records = sqlx::query_as::<_, ConversationRecord>(
            r#"
            SELECT
                id, context_id, timestamp, content, message_type,
                created_at, updated_at
            FROM conversations
            WHERE context_id = $1
                AND message_type = 'execution_summary'
            ORDER BY id DESC
            LIMIT $2
            "#,
        )
        .bind(self.context_id)
        .bind(self.limit)
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        Ok(records)
    }
}

/// Update memory access metrics
pub struct UpdateMemoryAccessCommand {
    pub memory_id: i64,
}

impl crate::commands::Command for UpdateMemoryAccessCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query(
            r#"
            UPDATE memories
            SET access_count = access_count + 1,
                last_accessed_at = NOW(),
                base_temporal_score = calculate_base_decay(
                    access_count + 1,
                    citation_count,
                    first_accessed_at,
                    NOW(),
                    relevance_score,
                    usefulness_score,
                    compression_level
                )
            WHERE id = $1
            "#,
        )
        .bind(self.memory_id)
        .execute(&app_state.db_pool)
        .await?;

        Ok(())
    }
}

/// Search memories with decay-adjusted scoring
#[derive(Debug)]
pub struct SearchMemoriesWithDecayQuery {
    pub query_embedding: Vec<f32>,
    pub limit: i64,
    pub time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryWithScore {
    pub memory: MemoryRecord,
    pub similarity_score: f64,
    pub decay_adjusted_score: f64,
}


impl Query for SearchMemoriesWithDecayQuery {
    type Output = Vec<MemoryWithScore>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let embedding = Vector::from(self.query_embedding.clone());

        let results = sqlx::query!(
            r#"
            SELECT
                id, content, embedding as "embedding: Vector", memory_category,
                base_temporal_score, access_count,
                first_accessed_at, last_accessed_at,
                citation_count, cross_context_value, learning_confidence,
                creation_context_id, last_reinforced_at,
                semantic_centrality, uniqueness_score,
                compression_level, compressed_content,
                context_decay_profile,
                created_at, updated_at,
                1 - (embedding <=> $1) as similarity_score
            FROM memories
            WHERE base_temporal_score > 0.1
                AND ($3::timestamptz IS NULL OR first_accessed_at >= $3)
                AND ($4::timestamptz IS NULL OR first_accessed_at <= $4)
            ORDER BY (1 - (embedding <=> $1)) * base_temporal_score DESC
            LIMIT $2
            "#,
            &embedding as &Vector,
            self.limit,
            self.time_range.as_ref().map(|(start, _)| *start),
            self.time_range.as_ref().map(|(_, end)| *end)
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        let mut memory_scores = Vec::new();
        for row in results {
            let memory = MemoryRecord {
                id: row.id,
                content: row.content,
                embedding: row.embedding,
                memory_category: row.memory_category,
                base_temporal_score: row.base_temporal_score.unwrap_or(0.0),
                access_count: row.access_count.unwrap_or(0),
                first_accessed_at: row.first_accessed_at.unwrap_or_else(|| Utc::now()),
                last_accessed_at: row.last_accessed_at.unwrap_or_else(|| Utc::now()),
                citation_count: row.citation_count.unwrap_or(0),
                cross_context_value: row.cross_context_value.unwrap_or(0.0),
                learning_confidence: row.learning_confidence.unwrap_or(0.0),
                creation_context_id: row.creation_context_id,
                last_reinforced_at: row.last_reinforced_at.unwrap_or_else(|| Utc::now()),
                semantic_centrality: row.semantic_centrality.unwrap_or(0.0),
                uniqueness_score: row.uniqueness_score.unwrap_or(0.0),
                compression_level: row.compression_level.unwrap_or(0),
                compressed_content: row.compressed_content,
                context_decay_profile: row.context_decay_profile.unwrap_or(serde_json::json!({})),
                created_at: row.created_at.unwrap_or_else(|| Utc::now()),
                updated_at: row.updated_at.unwrap_or_else(|| Utc::now()),
            };

            let similarity_score = row.similarity_score.unwrap_or(0.0);
            let decay_adjusted_score = similarity_score * memory.base_temporal_score;

            memory_scores.push(MemoryWithScore {
                memory,
                similarity_score,
                decay_adjusted_score,
            });
        }

        Ok(memory_scores)
    }
}

#[derive(Debug)]
pub struct SearchConversationsQuery {
    pub context_id: i64,
    pub limit: i64,
}

impl Query for SearchConversationsQuery {
    type Output = Vec<ConversationRecord>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let results = sqlx::query_as::<_, ConversationRecord>(
            r#"
            SELECT
                id, context_id, timestamp, content, message_type,
                created_at, updated_at
            FROM conversations
            WHERE context_id = $1
                AND message_type != 'execution_summary'
            ORDER BY updated_at DESC
            LIMIT $2
            "#,
        )
        .bind(self.context_id)
        .bind(self.limit)
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        Ok(results)
    }
}

pub struct GetAllMemoryBoundariesQuery;

impl Query for GetAllMemoryBoundariesQuery {
    type Output = Vec<MemoryBoundaries>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let rows = sqlx::query(
            r#"
            SELECT
                context_id,
                max_conversations,
                max_memories_per_category,
                compression_threshold_days,
                eviction_threshold_score,
                created_at,
                updated_at
            FROM memory_boundaries
            "#,
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        let mut boundaries = Vec::new();
        for row in rows {
            boundaries.push(MemoryBoundaries {
                context_id: row.try_get("context_id")?,
                max_conversations: row.try_get("max_conversations")?,
                max_memories_per_category: row.try_get("max_memories_per_category")?,
                compression_threshold_days: row.try_get("compression_threshold_days")?,
                eviction_threshold_score: row.try_get("eviction_threshold_score")?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
            });
        }

        Ok(boundaries)
    }
}

pub struct GetMemoryBoundariesQuery {
    pub context_id: i64,
}

impl Query for GetMemoryBoundariesQuery {
    type Output = MemoryBoundaries;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query(
            r#"
            SELECT
                context_id,
                max_conversations,
                max_memories_per_category,
                compression_threshold_days,
                eviction_threshold_score,
                created_at,
                updated_at
            FROM memory_boundaries
            WHERE context_id = $1
            "#,
        )
        .bind(self.context_id)
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
