use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::ai_knowledge_base::DocumentChunkSearchResult;

use pgvector::HalfVector;
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Serialize)]
struct EmbedContentRequest {
    model: String,
    content: Content,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_dimensionality: Option<i32>,
}

#[derive(Serialize)]
struct BatchEmbedContentsRequest {
    requests: Vec<EmbedContentRequestItem>,
}

#[derive(Serialize)]
struct EmbedContentRequestItem {
    model: String,
    content: Content,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_dimensionality: Option<i32>,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct Part {
    text: String,
}

#[derive(Deserialize)]
struct EmbedContentResponse {
    embedding: Embedding,
}

#[derive(Deserialize)]
struct BatchEmbedContentsResponse {
    embeddings: Vec<Embedding>,
}

#[derive(Deserialize)]
struct Embedding {
    values: Vec<f32>,
}

#[derive(Clone)]
pub struct GenerateEmbeddingCommand {
    pub text: String,
    pub task_type: Option<String>,
}

impl GenerateEmbeddingCommand {
    pub fn new(text: String) -> Self {
        Self {
            text,
            task_type: None,
        }
    }

    pub fn with_task_type(mut self, task_type: String) -> Self {
        self.task_type = Some(task_type);
        self
    }

    pub async fn execute_with(
        self,
        client: &reqwest::Client,
        api_key: &str,
        model: &str,
    ) -> Result<Vec<f32>, AppError> {
        let request = EmbedContentRequest {
            model: model.to_string(),
            content: Content {
                parts: vec![Part { text: self.text }],
            },
            task_type: self.task_type.or(Some("RETRIEVAL_DOCUMENT".to_string())),
            output_dimensionality: Some(3072),
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/{}:embedContent",
            model
        );

        let response = client
            .post(&url)
            .header("x-goog-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to send embedding request: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Embedding API error: {}",
                error_text
            )));
        }

        let embed_response: EmbedContentResponse = response.json().await.map_err(|e| {
            AppError::Internal(format!("Failed to parse embedding response: {}", e))
        })?;

        Ok(embed_response.embedding.values)
    }
}

impl Command for GenerateEmbeddingCommand {
    type Output = Vec<f32>;

    async fn execute(self, _app_state: &AppState) -> Result<Self::Output, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY is not set".to_string()))?;
        let model = std::env::var("GEMINI_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "models/gemini-embedding-001".to_string());
        let client = reqwest::Client::new();
        self.execute_with(&client, &api_key, &model).await
    }
}

#[derive(Clone)]
pub struct GenerateEmbeddingsCommand {
    pub texts: Vec<String>,
    pub task_type: Option<String>,
}

impl GenerateEmbeddingsCommand {
    pub fn new(texts: Vec<String>) -> Self {
        Self {
            texts,
            task_type: None,
        }
    }

    pub fn with_task_type(mut self, task_type: String) -> Self {
        self.task_type = Some(task_type);
        self
    }

    pub async fn execute_with(
        self,
        client: &reqwest::Client,
        api_key: &str,
        model: &str,
    ) -> Result<Vec<Vec<f32>>, AppError> {
        if self.texts.is_empty() {
            return Ok(vec![]);
        }

        const BATCH_SIZE: usize = 100;
        let mut all_embeddings = Vec::new();

        for chunk in self.texts.chunks(BATCH_SIZE) {
            let requests: Vec<EmbedContentRequestItem> = chunk
                .iter()
                .map(|text| EmbedContentRequestItem {
                    model: model.to_string(),
                    content: Content {
                        parts: vec![Part { text: text.clone() }],
                    },
                    task_type: self
                        .task_type
                        .clone()
                        .or(Some("RETRIEVAL_DOCUMENT".to_string())),
                    output_dimensionality: Some(3072),
                })
                .collect();

            let batch_request = BatchEmbedContentsRequest { requests };
            let url = format!(
                "https://generativelanguage.googleapis.com/v1/{}:batchEmbedContents",
                model
            );

            let response = client
                .post(&url)
                .header("x-goog-api-key", api_key)
                .header("Content-Type", "application/json")
                .json(&batch_request)
                .send()
                .await
                .map_err(|e| {
                    AppError::Internal(format!("Failed to send batch embedding request: {}", e))
                })?;

            if !response.status().is_success() {
                let status = response.status();
                let error_text = response.text().await.unwrap_or_default();
                tracing::error!(
                    "Batch embedding API error - Status: {}, URL: {}, Error: {}",
                    status,
                    url,
                    error_text
                );
                return Err(AppError::Internal(format!(
                    "Batch embedding API error ({}): {}",
                    status, error_text
                )));
            }

            let batch_response: BatchEmbedContentsResponse =
                response.json().await.map_err(|e| {
                    AppError::Internal(format!("Failed to parse batch embedding response: {}", e))
                })?;

            all_embeddings.extend(batch_response.embeddings.into_iter().map(|e| e.values));
        }

        Ok(all_embeddings)
    }
}

impl Command for GenerateEmbeddingsCommand {
    type Output = Vec<Vec<f32>>;

    async fn execute(self, _app_state: &AppState) -> Result<Self::Output, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY is not set".to_string()))?;
        let model = std::env::var("GEMINI_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "models/gemini-embedding-001".to_string());
        let client = reqwest::Client::new();
        self.execute_with(&client, &api_key, &model).await
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

    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
    ) -> Result<Vec<DocumentChunkSearchResult>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let query_embedding = HalfVector::from_f32_slice(&self.query_embedding);
        let max_distance = 1.2_f64;

        let rows = sqlx::query(
            r#"
            SELECT
                kbc.document_id,
                kbc.knowledge_base_id,
                kbc.content,
                kbc.chunk_index,
                (kbc.embedding::vector(3072) <-> $1)::float8 as score,
                d.title as document_title,
                d.description as document_description
            FROM knowledge_base_document_chunks kbc
            LEFT JOIN ai_knowledge_base_documents d ON kbc.document_id = d.id
            WHERE kbc.knowledge_base_id = ANY($2)
              AND (kbc.embedding::vector(3072) <-> $1) <= $4
            ORDER BY score ASC
            LIMIT $3
            "#,
        )
        .bind(query_embedding)
        .bind(&self.knowledge_base_ids)
        .bind(self.limit as i64)
        .bind(max_distance)
        .fetch_all(&mut *conn)
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
                document_description: row
                    .try_get("document_description")
                    .map_err(AppError::from)?,
            });
        }

        Ok(results)
    }
}

impl Command for SearchKnowledgeBaseEmbeddingsCommand {
    type Output = Vec<DocumentChunkSearchResult>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}
