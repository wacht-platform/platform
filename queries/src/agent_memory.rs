use crate::Query;
use chrono::{DateTime, Utc};
use common::error::AppError;
use common::state::AppState;
use models::{ConversationRecord, MemoryBoundaries, MemoryRecord};
use pgvector::HalfVector;
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
                creation_context_id, last_reinforced_at,
                semantic_centrality, uniqueness_score,
                compression_level, compressed_content,
                context_decay_profile,
                created_at, updated_at
            FROM memories
            WHERE memory_category = 'working'
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
}

impl GetLLMConversationHistoryQuery {
    pub fn new(context_id: i64) -> Self {
        Self { context_id }
    }
}

impl Query for GetLLMConversationHistoryQuery {
    type Output = Vec<ConversationRecord>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let records = sqlx::query_as::<_, ConversationRecord>(
            r#"
            WITH last_summary AS (
                -- Find the most recent execution summary using snowflake ID ordering
                SELECT id as last_summary_id
                FROM conversations
                WHERE context_id = $1
                  AND message_type = 'execution_summary'
                ORDER BY id DESC
                LIMIT 1
            ),
            last_summary_with_default AS (
                -- Ensure we always have a row, even if no summaries exist
                SELECT COALESCE(last_summary_id, 0) as last_summary_id
                FROM (SELECT 1) dummy
                LEFT JOIN last_summary ON TRUE
            ),
            recent_unsummarized AS (
                -- Get ALL conversations after the last summary (unbounded)
                SELECT c.id, c.context_id, c.timestamp, c.content, c.message_type,
                       c.token_count, c.created_at, c.updated_at
                FROM conversations c, last_summary_with_default ls
                WHERE c.context_id = $1
                  AND c.id > ls.last_summary_id
            ),
            execution_summaries AS (
                -- Get execution summaries with running totals and limits
                SELECT c.*,
                       ROW_NUMBER() OVER (ORDER BY c.id DESC) as execution_count,
                       SUM(c.token_count) OVER (ORDER BY c.id DESC) as running_tokens
                FROM conversations c
                WHERE c.context_id = $1
                  AND c.message_type = 'execution_summary'
                ORDER BY c.id DESC
            ),
            limited_summaries AS (
                -- Apply limits only to execution summaries
                SELECT id, context_id, timestamp, content, message_type,
                       token_count, created_at, updated_at
                FROM execution_summaries
                WHERE execution_count <= 20  -- Max 20 executions
                  AND running_tokens <= 40000  -- Token limit for summaries only
            )
            -- Combine both: all recent unsummarized + limited summaries
            SELECT * FROM recent_unsummarized
            UNION ALL
            SELECT * FROM limited_summaries
            ORDER BY id ASC  -- Return in chronological order for LLM
            "#,
        )
        .bind(self.context_id)
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        Ok(records)
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
        let embedding = HalfVector::from_f32_slice(&self.query_embedding);

        let results = sqlx::query!(
            r#"
            SELECT
                id, content, embedding as "embedding: HalfVector", memory_category,
                base_temporal_score, access_count,
                first_accessed_at, last_accessed_at,
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
            &embedding as &HalfVector,
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
