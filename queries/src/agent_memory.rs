use crate::Query;
use chrono::Utc;
use common::error::AppError;
use common::state::AppState;
use dto::json::agent_memory::MemoryCategory;
use models::{ConversationRecord, MemoryBoundaries, MemoryRecord};
use pgvector::HalfVector;
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Debug)]
pub struct GetMRUMemoriesQuery {
    pub context_id: i64,
    pub limit: i64,
}

impl GetMRUMemoriesQuery {
    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<Vec<MemoryRecord>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let records = sqlx::query_as::<_, MemoryRecord>(
            r#"
            SELECT
                id, content, embedding, memory_category,
                base_temporal_score, access_count,
                first_accessed_at, last_accessed_at,
                creation_context_id, agent_id, last_reinforced_at,
                semantic_centrality, uniqueness_score,
                compression_level, compressed_content,
                created_at, updated_at
            FROM memories
            WHERE creation_context_id = $1
            ORDER BY last_accessed_at DESC
            LIMIT $2
            "#,
        )
        .bind(self.context_id)
        .bind(self.limit)
        .fetch_all(&mut *conn)
        .await
        .map_err(AppError::from)?;

        Ok(records)
    }
}

impl Query for GetMRUMemoriesQuery {
    type Output = Vec<MemoryRecord>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
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
pub struct GetConversationByIdQuery {
    pub conversation_id: i64,
}

impl GetConversationByIdQuery {
    pub fn new(conversation_id: i64) -> Self {
        Self { conversation_id }
    }
}

impl Query for GetConversationByIdQuery {
    type Output = ConversationRecord;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let record = sqlx::query_as::<_, ConversationRecord>(
            r#"
            SELECT
                id, context_id, timestamp, content, message_type,
                created_at, updated_at
            FROM conversations
            WHERE id = $1
            "#,
        )
        .bind(self.conversation_id)
        .fetch_optional(&app_state.db_pool)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| {
            AppError::NotFound(format!("Conversation {} not found", self.conversation_id))
        })?;

        Ok(record)
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
    pub context_id: Option<i64>,
    pub agent_id: Option<i64>,
    pub categories: Option<Vec<MemoryCategory>>,
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

        // Convert categories to strings for SQL
        let category_strings: Option<Vec<String>> = self
            .categories
            .as_ref()
            .map(|cats| cats.iter().map(|c| c.to_string()).collect());

        let results = sqlx::query!(
            r#"
            SELECT
                id, content, embedding as "embedding: HalfVector", memory_category,
                base_temporal_score, access_count,
                first_accessed_at, last_accessed_at,
                creation_context_id, agent_id, last_reinforced_at,
                semantic_centrality, uniqueness_score,
                compression_level, compressed_content,
                created_at, updated_at,
                1 - (embedding <=> $1) as similarity_score
            FROM memories
            WHERE base_temporal_score > 0.1
                AND (
                    ($3::bigint IS NULL AND $4::bigint IS NULL) OR
                    ($3::bigint IS NOT NULL AND creation_context_id = $3) OR
                    ($4::bigint IS NOT NULL AND agent_id = $4)
                )
                AND ($5::text[] IS NULL OR memory_category = ANY($5))
            ORDER BY (1 - (embedding <=> $1)) * base_temporal_score * (1 + LN(1 + access_count)) DESC
            LIMIT $2
            "#,
            &embedding as &HalfVector,
            self.limit,
            self.context_id,
            self.agent_id,
            category_strings.as_deref()
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
                agent_id: row.agent_id,
                last_reinforced_at: row.last_reinforced_at.unwrap_or_else(|| Utc::now()),
                semantic_centrality: row.semantic_centrality.unwrap_or(0.0),
                uniqueness_score: row.uniqueness_score.unwrap_or(0.0),
                compression_level: row.compression_level.unwrap_or(0),
                compressed_content: row.compressed_content,
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

/// Find memories similar to given embedding for deduplication
#[derive(Debug)]
pub struct FindSimilarMemoriesQuery {
    pub agent_id: i64,
    pub embedding: Vec<f32>,
    pub threshold: f64,
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarMemory {
    pub id: i64,
    pub content: String,
    pub similarity: f64,
}

impl Query for FindSimilarMemoriesQuery {
    type Output = Vec<SimilarMemory>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let embedding = HalfVector::from_f32_slice(&self.embedding);

        let results = sqlx::query!(
            r#"
            SELECT id, content, 1 - (embedding <=> $1) as similarity
            FROM memories
            WHERE agent_id = $2
              AND 1 - (embedding <=> $1) > $3
            ORDER BY similarity DESC
            LIMIT $4
            "#,
            &embedding as &HalfVector,
            self.agent_id,
            self.threshold,
            self.limit
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        Ok(results
            .into_iter()
            .map(|r| SimilarMemory {
                id: r.id,
                content: r.content,
                similarity: r.similarity.unwrap_or(0.0),
            })
            .collect())
    }
}

/// Get a single memory by ID
#[derive(Debug)]
pub struct GetMemoryByIdQuery {
    pub memory_id: i64,
}

impl Query for GetMemoryByIdQuery {
    type Output = MemoryRecord;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query_as::<_, MemoryRecord>(
            r#"
            SELECT
                id, content, embedding, memory_category,
                base_temporal_score, access_count,
                first_accessed_at, last_accessed_at,
                creation_context_id, agent_id, last_reinforced_at,
                semantic_centrality, uniqueness_score,
                compression_level, compressed_content,
                created_at, updated_at
            FROM memories
            WHERE id = $1
            "#,
        )
        .bind(self.memory_id)
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(AppError::from)
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

// New queries for memory loading with scopes

pub struct GetSessionMemoriesQuery {
    pub context_id: i64,
    pub categories: Option<Vec<MemoryCategory>>,
    pub limit: i64,
}

impl Query for GetSessionMemoriesQuery {
    type Output = Vec<MemoryRecord>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let category_strings: Option<Vec<String>> = self
            .categories
            .as_ref()
            .map(|cats| cats.iter().map(|c| c.to_string()).collect());

        let records = sqlx::query_as::<_, MemoryRecord>(
            r#"
            SELECT
                id, content, embedding, memory_category,
                base_temporal_score, access_count,
                first_accessed_at, last_accessed_at,
                creation_context_id, agent_id, last_reinforced_at,
                semantic_centrality, uniqueness_score,
                compression_level, compressed_content,
                created_at, updated_at
            FROM memories
            WHERE creation_context_id = $1
                AND ($2::text[] IS NULL OR memory_category = ANY($2))
            ORDER BY base_temporal_score * (1.0 / (1.0 + EXTRACT(EPOCH FROM (NOW() - last_accessed_at)) / 86400)) * (1 + LN(1 + access_count)) DESC
            LIMIT $3
            "#,
        )
        .bind(self.context_id)
        .bind(category_strings.as_deref())
        .bind(self.limit)
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        Ok(records)
    }
}

pub struct GetAgentMemoriesQuery {
    pub agent_id: i64,
    pub categories: Option<Vec<MemoryCategory>>,
    pub limit: i64,
}

impl Query for GetAgentMemoriesQuery {
    type Output = Vec<MemoryRecord>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let category_strings: Option<Vec<String>> = self
            .categories
            .as_ref()
            .map(|cats| cats.iter().map(|c| c.to_string()).collect());

        let records = sqlx::query_as::<_, MemoryRecord>(
            r#"
            SELECT
                id, content, embedding, memory_category,
                base_temporal_score, access_count,
                first_accessed_at, last_accessed_at,
                creation_context_id, agent_id, last_reinforced_at,
                semantic_centrality, uniqueness_score,
                compression_level, compressed_content,
                created_at, updated_at
            FROM memories
            WHERE agent_id = $1
                AND creation_context_id IS NULL
                AND ($2::text[] IS NULL OR memory_category = ANY($2))
            ORDER BY base_temporal_score * semantic_centrality * (1 + LN(1 + access_count)) DESC
            LIMIT $3
            "#,
        )
        .bind(self.agent_id)
        .bind(category_strings.as_deref())
        .bind(self.limit)
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        Ok(records)
    }
}

pub struct GetAgentImportantMemoriesQuery {
    pub agent_id: i64,
    pub categories: Option<Vec<MemoryCategory>>,
    pub min_importance: f64,
    pub limit: i64,
}

impl Query for GetAgentImportantMemoriesQuery {
    type Output = Vec<MemoryRecord>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let category_strings: Option<Vec<String>> = self
            .categories
            .as_ref()
            .map(|cats| cats.iter().map(|c| c.to_string()).collect());

        let records = sqlx::query_as::<_, MemoryRecord>(
            r#"
            SELECT
                id, content, embedding, memory_category,
                base_temporal_score, access_count,
                first_accessed_at, last_accessed_at,
                creation_context_id, agent_id, last_reinforced_at,
                semantic_centrality, uniqueness_score,
                compression_level, compressed_content,
                created_at, updated_at
            FROM memories
            WHERE agent_id = $1
                AND base_temporal_score >= $2
                AND ($3::text[] IS NULL OR memory_category = ANY($3))
            ORDER BY base_temporal_score * uniqueness_score * (1 + LN(1 + access_count)) DESC
            LIMIT $4
            "#,
        )
        .bind(self.agent_id)
        .bind(self.min_importance)
        .bind(category_strings.as_deref())
        .bind(self.limit)
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        Ok(records)
    }
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
