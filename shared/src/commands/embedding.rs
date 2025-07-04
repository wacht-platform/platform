use super::Command;
use crate::{
    error::AppError, models::ai_knowledge_base::DocumentChunkSearchResult, state::AppState,
};
use llm::builder::{LLMBackend, LLMBuilder};
use pgvector::Vector;
use sqlx::Row;

#[derive(Clone)]
pub struct GenerateEmbeddingCommand {
    pub text: String,
}

impl GenerateEmbeddingCommand {
    pub fn new(text: String) -> Self {
        Self { text }
    }
}

impl Command for GenerateEmbeddingCommand {
    type Output = Vec<f32>;

    async fn execute(self, _app_state: &AppState) -> Result<Self::Output, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY").map_err(|_| {
            AppError::Internal("GEMINI_API_KEY environment variable not set".to_string())
        })?;

        let model = std::env::var("GEMINI_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "text-embedding-004".to_string());

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model(&model)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to initialize Gemini LLM: {}", e)))?;

        let embeddings = llm
            .embed(vec![self.text])
            .await
            .map_err(|e| AppError::Internal(format!("Failed to generate embeddings: {}", e)))?;

        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Internal("No embedding returned".to_string()))
    }
}

#[derive(Clone)]
pub struct GenerateEmbeddingsCommand {
    pub texts: Vec<String>,
}

impl GenerateEmbeddingsCommand {
    pub fn new(texts: Vec<String>) -> Self {
        Self { texts }
    }
}

impl Command for GenerateEmbeddingsCommand {
    type Output = Vec<Vec<f32>>;

    async fn execute(self, _app_state: &AppState) -> Result<Self::Output, AppError> {
        if self.texts.is_empty() {
            return Ok(vec![]);
        }

        let api_key = std::env::var("GEMINI_API_KEY").map_err(|_| {
            AppError::Internal("GEMINI_API_KEY environment variable not set".to_string())
        })?;

        let model = std::env::var("GEMINI_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "text-embedding-004".to_string());

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model(&model)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to initialize Gemini LLM: {}", e)))?;

        let embeddings = llm
            .embed(self.texts)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to generate embeddings: {}", e)))?;

        Ok(embeddings)
    }
}

#[derive(Clone)]
pub struct SearchKnowledgeBaseEmbeddingsCommand {
    pub knowledge_base_id: i64,
    pub query_embedding: Vec<f32>,
    pub limit: u64,
}

impl SearchKnowledgeBaseEmbeddingsCommand {
    pub fn new(knowledge_base_id: i64, query_embedding: Vec<f32>, limit: u64) -> Self {
        Self {
            knowledge_base_id,
            query_embedding,
            limit,
        }
    }
}

impl Command for SearchKnowledgeBaseEmbeddingsCommand {
    type Output = Vec<DocumentChunkSearchResult>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let query_embedding = Vector::from(self.query_embedding.clone());
        let rows = sqlx::query(
            r#"
            SELECT document_id, knowledge_base_id, content, chunk_index, (embedding <-> $1)::float8 as score
            FROM knowledge_base_document_chunks
            WHERE knowledge_base_id = $2
            ORDER BY score ASC LIMIT $3
            "#,
        )
        .bind(query_embedding)
        .bind(self.knowledge_base_id)
        .bind(self.limit as i64)
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        let mut results = Vec::new();
        for row in rows {
            results.push(DocumentChunkSearchResult {
                document_id: row.try_get("document_id").map_err(AppError::from)?,
                knowledge_base_id: row.try_get("knowledge_base_id").map_err(AppError::from)?,
                content: row.try_get("content").map_err(AppError::from)?,
                score: row.try_get("score").map_err(AppError::from)?,
                chunk_index: row.try_get("chunk_index").map_err(AppError::from)?,
            });
        }

        Ok(results)
    }
}
