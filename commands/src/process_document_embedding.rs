use crate::{Command, GenerateEmbeddingsCommand};
use chrono::Utc;
use common::error::AppError;
use common::state::AppState;
use pgvector::HalfVector;
use std::future::Future;
use tracing::{error, info};

pub struct ProcessDocumentBatchCommand {
    pub deployment_id: i64,
    pub knowledge_base_id: i64,
    pub batch_size: usize,
}

impl ProcessDocumentBatchCommand {
    pub fn new(deployment_id: i64, knowledge_base_id: i64, batch_size: usize) -> Self {
        Self {
            deployment_id,
            knowledge_base_id,
            batch_size,
        }
    }
}

impl Command for ProcessDocumentBatchCommand {
    type Output = String;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer(), |texts| async move {
            let api_key = std::env::var("GEMINI_API_KEY")
                .map_err(|_| AppError::Internal("GEMINI_API_KEY is not set".to_string()))?;
            let model = std::env::var("GEMINI_EMBEDDING_MODEL")
                .unwrap_or_else(|_| "models/gemini-embedding-001".to_string());
            let client = reqwest::Client::new();
            let embeddings_command = GenerateEmbeddingsCommand::new(texts);
            embeddings_command
                .execute_with(&client, &api_key, &model)
                .await
        })
        .await
    }
}

impl ProcessDocumentBatchCommand {
    pub async fn execute_with<'a, A, GenerateEmbeddingsFn, GenerateEmbeddingsFut>(
        self,
        acquirer: A,
        generate_embeddings: GenerateEmbeddingsFn,
    ) -> Result<String, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
        GenerateEmbeddingsFn: Fn(Vec<String>) -> GenerateEmbeddingsFut + Copy,
        GenerateEmbeddingsFut: Future<Output = Result<Vec<Vec<f32>>, AppError>>,
    {
        let mut conn = acquirer.acquire().await?;
        info!(
            "Processing embeddings for up to {} chunks in knowledge base {} (deployment {})",
            self.batch_size, self.knowledge_base_id, self.deployment_id
        );

        let pending_chunks = sqlx::query!(
            r#"
            SELECT document_id, chunk_index, content 
            FROM knowledge_base_document_chunks 
            WHERE knowledge_base_id = $1 
            AND deployment_id = $2
            AND embedding IS NULL
            ORDER BY document_id, chunk_index
            LIMIT $3
            "#,
            self.knowledge_base_id,
            self.deployment_id,
            self.batch_size as i64
        )
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| AppError::Database(e))?;

        if pending_chunks.is_empty() {
            return Ok("No pending chunks found to process".to_string());
        }

        info!("Found {} chunks without embeddings", pending_chunks.len());

        // Process embeddings in batches of 100
        let mut total_processed = 0;
        let mut documents_with_embeddings: std::collections::HashSet<i64> =
            std::collections::HashSet::new();

        for chunk_batch in pending_chunks.chunks(100) {
            let chunk_texts: Vec<String> = chunk_batch
                .iter()
                .map(|chunk| chunk.content.clone())
                .collect();

            let embeddings = match generate_embeddings(chunk_texts).await {
                Ok(embeddings) => embeddings,
                Err(e) => {
                    error!("Failed to generate embeddings for batch: {}", e);
                    continue;
                }
            };

            // Update chunks with embeddings
            for (chunk, embedding) in chunk_batch.iter().zip(embeddings.into_iter()) {
                let embedding_vector = HalfVector::from_f32_slice(&embedding);

                match sqlx::query(
                    "UPDATE knowledge_base_document_chunks SET embedding = $1, updated_at = $2 WHERE document_id = $3 AND chunk_index = $4"
                )
                .bind(embedding_vector)
                .bind(Utc::now())
                .bind(chunk.document_id)
                .bind(chunk.chunk_index)
                .execute(&mut *conn)
                .await {
                    Ok(_) => {
                        total_processed += 1;
                        documents_with_embeddings.insert(chunk.document_id);
                    }
                    Err(e) => {
                        error!("Failed to update embedding for chunk {}:{}: {}", chunk.document_id, chunk.chunk_index, e);
                    }
                }
            }
        }

        // Update document status for completed documents
        let documents_count = documents_with_embeddings.len();
        for document_id in documents_with_embeddings {
            // Check if all chunks for this document now have embeddings
            let remaining_chunks = sqlx::query!(
                "SELECT COUNT(*) as count FROM knowledge_base_document_chunks WHERE document_id = $1 AND embedding IS NULL",
                document_id
            )
            .fetch_one(&mut *conn)
            .await
            .map_err(|e| AppError::Database(e))?;

            if remaining_chunks.count.unwrap_or(1) == 0 {
                // All chunks have embeddings, mark document as completed
                if let Err(e) = sqlx::query!(
                    r#"
                    UPDATE ai_knowledge_base_documents 
                    SET processing_metadata = jsonb_set(
                        COALESCE(processing_metadata, '{}'),
                        '{status}',
                        '"completed"'
                    ),
                    updated_at = $1
                    WHERE id = $2
                    "#,
                    Utc::now(),
                    document_id
                )
                .execute(&mut *conn)
                .await
                {
                    error!(
                        "Failed to update document {} status to completed: {}",
                        document_id, e
                    );
                }
            }
        }

        Ok(format!(
            "Processed embeddings for {} chunks across {} documents",
            total_processed, documents_count
        ))
    }
}
