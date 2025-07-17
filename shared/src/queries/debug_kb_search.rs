use crate::{error::AppError, state::AppState};
use super::Query;

/// Debug query to check KB contents
pub struct DebugKnowledgeBaseContentQuery {
    pub knowledge_base_id: i64,
    pub deployment_id: i64,
}

#[derive(Debug, Clone)]
pub struct KbDebugInfo {
    pub total_chunks: i64,
    pub chunks_with_vectors: i64,
    pub chunks_without_vectors: i64,
    pub sample_content: Vec<String>,
}

impl Query for DebugKnowledgeBaseContentQuery {
    type Output = KbDebugInfo;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let pool = &app_state.db_pool;

        // Get total chunks
        let total_chunks = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM knowledge_base_document_chunks WHERE knowledge_base_id = $1 AND deployment_id = $2"
        )
        .bind(self.knowledge_base_id)
        .bind(self.deployment_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        // Get chunks with vectors
        let chunks_with_vectors = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM knowledge_base_document_chunks WHERE knowledge_base_id = $1 AND deployment_id = $2 AND search_vector IS NOT NULL"
        )
        .bind(self.knowledge_base_id)
        .bind(self.deployment_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        let chunks_without_vectors = total_chunks - chunks_with_vectors;

        // Get sample content
        let sample_rows: Vec<(String,)> = sqlx::query_as(
            "SELECT content FROM knowledge_base_document_chunks WHERE knowledge_base_id = $1 AND deployment_id = $2 LIMIT 3"
        )
        .bind(self.knowledge_base_id)
        .bind(self.deployment_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        let sample_content = sample_rows
            .into_iter()
            .map(|(content,)| content)
            .collect();

        Ok(KbDebugInfo {
            total_chunks,
            chunks_with_vectors,
            chunks_without_vectors,
            sample_content,
        })
    }
}

/// Debug query to test text search
pub struct DebugTextSearchQuery {
    pub knowledge_base_id: i64,
    pub deployment_id: i64,
    pub search_term: String,
}

#[derive(Debug, Clone)]
pub struct TextSearchDebugResult {
    pub matching_chunks: i64,
    pub sample_matches: Vec<String>,
}

impl Query for DebugTextSearchQuery {
    type Output = TextSearchDebugResult;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let pool = &app_state.db_pool;

        // Count matching chunks
        let matching_chunks = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM knowledge_base_document_chunks 
             WHERE knowledge_base_id = $1 AND deployment_id = $2 
             AND content ILIKE $3"
        )
        .bind(self.knowledge_base_id)
        .bind(self.deployment_id)
        .bind(format!("%{}%", self.search_term))
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        // Get sample matches
        let sample_rows: Vec<(String,)> = sqlx::query_as(
            "SELECT content FROM knowledge_base_document_chunks 
             WHERE knowledge_base_id = $1 AND deployment_id = $2 
             AND content ILIKE $3
             LIMIT 3"
        )
        .bind(self.knowledge_base_id)
        .bind(self.deployment_id)
        .bind(format!("%{}%", self.search_term))
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        let sample_matches = sample_rows
            .into_iter()
            .map(|(content,)| content)
            .collect();

        Ok(TextSearchDebugResult {
            matching_chunks,
            sample_matches,
        })
    }
}