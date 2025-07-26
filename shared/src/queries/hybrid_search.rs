use crate::{error::AppError, state::AppState};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::Query;

/// Result from hybrid search for knowledge base chunks
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct HybridSearchKbResult {
    pub document_id: i64,
    pub knowledge_base_id: i64,
    pub chunk_index: i32,
    pub content: String,
    pub document_title: Option<String>,
    pub document_description: Option<String>,
    pub vector_similarity: f64,  // double precision in PostgreSQL
    pub text_rank: f64,          // double precision in PostgreSQL
    pub combined_score: f64,     // double precision in PostgreSQL
}

/// Query for hybrid search in knowledge base
pub struct HybridSearchKnowledgeBaseQuery {
    pub query_text: String,
    pub query_embedding: Vec<f32>,
    pub knowledge_base_ids: Vec<i64>,
    pub deployment_id: i64,
    pub max_results: i32,
    pub vector_weight: f64,
    pub text_weight: f64,
}

impl Query for HybridSearchKnowledgeBaseQuery {
    type Output = Vec<HybridSearchKbResult>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let pool = &app_state.db_pool;

        // Convert Vec<f32> to pgvector format string
        let embedding_str = format!(
            "[{}]",
            self.query_embedding
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );

        tracing::info!(
            "Executing hybrid search - Query: '{}', KB IDs: {:?}, Deployment ID: {}, Max results: {}, Weights: vector={}, text={}",
            self.query_text,
            self.knowledge_base_ids,
            self.deployment_id,
            self.max_results,
            self.vector_weight,
            self.text_weight
        );

        let results = sqlx::query_as::<_, HybridSearchKbResult>(
            r#"
            WITH vector_search AS (
                SELECT 
                    kbc.document_id,
                    kbc.knowledge_base_id,
                    kbc.chunk_index,
                    kbc.content,
                    d.title as document_title,
                    d.description as document_description,
                    (kbc.embedding <-> $2::vector(768))::float8 as vector_distance
                FROM knowledge_base_document_chunks kbc
                LEFT JOIN ai_knowledge_base_documents d ON kbc.document_id = d.id
                WHERE kbc.knowledge_base_id = ANY($3)
                    AND kbc.deployment_id = $4
                ORDER BY vector_distance ASC
                LIMIT ($5 * 2)  -- Double the limit to account for potential overlap
            ),
            text_search AS (
                SELECT 
                    kbc.document_id,
                    kbc.knowledge_base_id,
                    kbc.chunk_index,
                    kbc.content,
                    d.title as document_title,
                    d.description as document_description,
                    ts_rank(kbc.search_vector, plainto_tsquery('english', $1))::float8 as text_rank
                FROM knowledge_base_document_chunks kbc
                LEFT JOIN ai_knowledge_base_documents d ON kbc.document_id = d.id
                WHERE kbc.knowledge_base_id = ANY($3)
                    AND kbc.deployment_id = $4
                    AND kbc.search_vector @@ plainto_tsquery('english', $1)
                ORDER BY text_rank DESC
                LIMIT ($5 * 2)  -- Double the limit to account for potential overlap
            ),
            combined AS (
                SELECT 
                    COALESCE(v.document_id, t.document_id) as document_id,
                    COALESCE(v.knowledge_base_id, t.knowledge_base_id) as knowledge_base_id,
                    COALESCE(v.chunk_index, t.chunk_index) as chunk_index,
                    COALESCE(v.content, t.content) as content,
                    COALESCE(v.document_title, t.document_title) as document_title,
                    COALESCE(v.document_description, t.document_description) as document_description,
                    -- Keep original naming but with correct default for distance
                    COALESCE(v.vector_distance, 2.0) as vector_similarity,  -- This is actually distance
                    COALESCE(t.text_rank, 0.0) as text_rank,
                    -- Normalize vector distance to similarity score (0-1, higher is better)
                    -- Distance ranges from 0 to 2, so similarity = 1 - (distance/2)
                    ((1.0 - COALESCE(v.vector_distance, 2.0)/2.0) * $6 + COALESCE(t.text_rank, 0.0) * $7) as combined_score
                FROM vector_search v
                FULL OUTER JOIN text_search t 
                    ON v.document_id = t.document_id AND v.chunk_index = t.chunk_index
            )
            SELECT * FROM combined
            ORDER BY combined_score DESC
            LIMIT $5
            "#
        )
        .bind(&self.query_text)
        .bind(&embedding_str)
        .bind(&self.knowledge_base_ids)
        .bind(self.deployment_id)
        .bind(self.max_results)
        .bind(self.vector_weight)
        .bind(self.text_weight)
        .fetch_all(pool)
        .await
        .map_err(|e| {
            tracing::error!("Hybrid search query failed: {}", e);
            AppError::Internal(format!("Failed to execute hybrid search: {}", e))
        })?;

        tracing::info!("Hybrid search returned {} results", results.len());
        
        for (idx, result) in results.iter().enumerate() {
            tracing::debug!(
                "Result {}: doc_id={}, chunk={}, title={:?}, desc={:?}, vector_sim={:.4}, text_rank={:.4}, combined={:.4}",
                idx,
                result.document_id,
                result.chunk_index,
                result.document_title,
                result.document_description,
                result.vector_similarity,
                result.text_rank,
                result.combined_score
            );
        }

        Ok(results)
    }
}

/// Result from hybrid search for memories
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct HybridSearchMemoryResult {
    pub id: i64,
    pub content: String,
    pub memory_type: String,
    pub importance: f64,
    pub vector_similarity: f64,
    pub text_rank: f64,
    pub combined_score: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Query for hybrid search in memories
pub struct HybridSearchMemoriesQuery {
    pub query_text: String,
    pub query_embedding: Vec<f32>,
    pub agent_id: i64,
    pub context_id: i64,
    pub max_results: i32,
    pub vector_weight: f64,
    pub text_weight: f64,
}

impl Query for HybridSearchMemoriesQuery {
    type Output = Vec<HybridSearchMemoryResult>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let pool = &app_state.db_pool;

        // Convert Vec<f32> to pgvector format string
        let embedding_str = format!(
            "[{}]",
            self.query_embedding
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );

        let results = sqlx::query_as::<_, HybridSearchMemoryResult>(
            r#"
            SELECT 
                id,
                content,
                memory_type,
                importance,
                vector_similarity,
                text_rank,
                combined_score,
                created_at
            FROM hybrid_search_memories(
                $1::TEXT,                    -- query text
                $2::vector(768),             -- query embedding
                $3::BIGINT,                  -- agent_id
                $4::BIGINT,                  -- context_id
                $5::INT,                     -- max_results
                $6::FLOAT,                   -- min_relevance
                $7::FLOAT,                   -- vector_weight
                $8::FLOAT                    -- text_weight
            )
            "#
        )
        .bind(&self.query_text)
        .bind(&embedding_str)
        .bind(self.agent_id)
        .bind(self.context_id)
        .bind(self.max_results)
        .bind(0.0_f64)  // min_relevance disabled - pass 0.0 to return all results
        .bind(self.vector_weight)
        .bind(self.text_weight)
        .fetch_all(pool)
        .await
        .map_err(|e| {
            tracing::error!("Hybrid memory search query failed: {}", e);
            AppError::Internal(format!("Failed to execute hybrid memory search: {}", e))
        })?;

        Ok(results)
    }
}

/// Query for pure full-text search in knowledge base
pub struct FullTextSearchKnowledgeBaseQuery {
    pub query_text: String,
    pub knowledge_base_ids: Vec<i64>,
    pub deployment_id: i64,
    pub max_results: i32,
}

impl Query for FullTextSearchKnowledgeBaseQuery {
    type Output = Vec<FullTextSearchResult>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let pool = &app_state.db_pool;

        let results = sqlx::query_as::<_, FullTextSearchResult>(
            r#"
            SELECT 
                kbc.document_id,
                kbc.knowledge_base_id,
                kbc.chunk_index,
                kbc.content,
                ts_rank(kbc.search_vector, plainto_tsquery('english', $1)) as text_rank,
                d.title as document_title,
                d.description as document_description
            FROM knowledge_base_document_chunks kbc
            LEFT JOIN ai_knowledge_base_documents d ON kbc.document_id = d.id
            WHERE kbc.knowledge_base_id = ANY($2)
              AND kbc.deployment_id = $3
              AND kbc.search_vector @@ plainto_tsquery('english', $1)
            ORDER BY text_rank DESC
            LIMIT $4
            "#
        )
        .bind(&self.query_text)
        .bind(&self.knowledge_base_ids)
        .bind(self.deployment_id)
        .bind(self.max_results)
        .fetch_all(pool)
        .await
        .map_err(|e| {
            tracing::error!("Full-text search query failed: {}", e);
            AppError::Internal(format!("Failed to execute full-text search: {}", e))
        })?;

        Ok(results)
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct FullTextSearchResult {
    pub document_id: i64,
    pub knowledge_base_id: i64,
    pub chunk_index: i32,
    pub content: String,
    pub text_rank: f32,
    pub document_title: Option<String>,
    pub document_description: Option<String>,
}