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
    pub knowledge_base_ids: Vec<i64>,
    pub query_embedding: Vec<f32>,
    pub limit: u64,
}

impl SearchKnowledgeBaseEmbeddingsCommand {
    pub fn new(knowledge_base_ids: Vec<i64>, query_embedding: Vec<f32>, limit: u64) -> Self {
        Self {
            knowledge_base_ids,
            query_embedding,
            limit,
        }
    }
}

impl Command for SearchKnowledgeBaseEmbeddingsCommand {
    type Output = Vec<DocumentChunkSearchResult>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let query_embedding = Vector::from(self.query_embedding.clone());
        // Set minimum similarity threshold - cosine distance of 1.1-1.2 means ~40-45% similarity
        // Cosine distance ranges from 0 (identical) to 2 (opposite)
        // Distance 1.0 = 50% similarity, 1.2 = 40% similarity
        let max_distance = 1.2_f64;
        
        let rows = sqlx::query(
            r#"
            SELECT 
                kbc.document_id, 
                kbc.knowledge_base_id, 
                kbc.content, 
                kbc.chunk_index, 
                (kbc.embedding <-> $1)::float8 as score,
                d.title as document_title,
                d.description as document_description
            FROM knowledge_base_document_chunks kbc
            LEFT JOIN ai_knowledge_base_documents d ON kbc.document_id = d.id
            WHERE kbc.knowledge_base_id = ANY($2)
              AND (kbc.embedding <-> $1) <= $4
            ORDER BY score ASC 
            LIMIT $3
            "#,
        )
        .bind(query_embedding)
        .bind(&self.knowledge_base_ids)
        .bind(self.limit as i64)
        .bind(max_distance)
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
                document_title: row.try_get("document_title").map_err(AppError::from)?,
                document_description: row.try_get("document_description").map_err(AppError::from)?,
            });
        }

        Ok(results)
    }
}
