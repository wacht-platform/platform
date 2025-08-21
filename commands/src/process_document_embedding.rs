use crate::{Command, GenerateEmbeddingsCommand};
use chrono::Utc;
use common::error::AppError;
use common::state::AppState;
use pgvector::HalfVector;
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
        info!(
            "Processing batch of up to {} pending documents for knowledge base {} in deployment {}",
            self.batch_size, self.knowledge_base_id, self.deployment_id
        );

        // Get pending documents from the knowledge base
        let pending_documents = sqlx::query!(
            r#"
            SELECT id, file_url, file_type, title
            FROM ai_knowledge_base_documents 
            WHERE knowledge_base_id = $1 
            AND (processing_metadata->>'status' = 'pending' OR processing_metadata IS NULL)
            ORDER BY created_at ASC
            LIMIT $2
            "#,
            self.knowledge_base_id,
            self.batch_size as i64
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        if pending_documents.is_empty() {
            return Ok("No pending documents found to process".to_string());
        }

        info!("Found {} pending documents to process", pending_documents.len());

        // First, extract text and create chunks for all documents
        let mut all_chunks: Vec<(i64, usize, String)> = Vec::new(); // (document_id, chunk_index, content)
        let mut document_chunk_counts: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
        let mut processed_count = 0;

        let pending_documents_len = pending_documents.len();
        for document in pending_documents {
            info!("Extracting text from document {} ({})", document.id, document.title);

            // Update document status to processing
            sqlx::query!(
                r#"
                UPDATE ai_knowledge_base_documents 
                SET processing_metadata = jsonb_set(
                    COALESCE(processing_metadata, '{}'),
                    '{status}',
                    '"processing"'
                ),
                updated_at = $1
                WHERE id = $2
                "#,
                Utc::now(),
                document.id
            )
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

            match self.extract_document_chunks(
                document.id,
                &document.file_url,
                &document.file_type,
                app_state,
            ).await {
                Ok(chunks) => {
                    let chunk_count = chunks.len();
                    document_chunk_counts.insert(document.id, chunk_count);
                    for (chunk_index, chunk_content) in chunks.into_iter().enumerate() {
                        all_chunks.push((document.id, chunk_index, chunk_content));
                    }
                    processed_count += 1;
                    info!("Successfully extracted {} chunks from document {}", chunk_count, document.id);
                }
                Err(e) => {
                    error!("Failed to extract chunks from document {}: {}", document.id, e);
                    
                    // Mark document as failed
                    let _ = sqlx::query!(
                        r#"
                        UPDATE ai_knowledge_base_documents 
                        SET processing_metadata = jsonb_set(
                            jsonb_set(
                                COALESCE(processing_metadata, '{}'),
                                '{status}',
                                '"failed"'
                            ),
                            '{error}',
                            $1::text::jsonb
                        ),
                        updated_at = $2
                        WHERE id = $3
                        "#,
                        e.to_string(),
                        Utc::now(),
                        document.id
                    )
                    .execute(&app_state.db_pool)
                    .await;
                }
            }
        }

        let total_chunks = all_chunks.len();
        info!("Extracted {} total chunks from {} documents. Processing embeddings in batches of 100.", total_chunks, processed_count);

        // Process embeddings in batches of 100
        let mut total_stored_chunks = 0;
        for chunk_batch in all_chunks.chunks(100) {
            let chunk_texts: Vec<String> = chunk_batch.iter().map(|(_, _, content)| content.clone()).collect();
            
            // Generate embeddings for this batch
            let embeddings_command = GenerateEmbeddingsCommand::new(chunk_texts);
            let embeddings = embeddings_command.execute(app_state).await?;

            // Store embeddings in database
            for ((document_id, chunk_index, chunk_content), embedding) in chunk_batch.iter().zip(embeddings.into_iter()) {
                let now = Utc::now();
                let embedding_vector = HalfVector::from_f32_slice(&embedding);

                match sqlx::query(
                    r#"
                    INSERT INTO knowledge_base_document_chunks
                    (document_id, knowledge_base_id, deployment_id, chunk_index, content, embedding, created_at, updated_at)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                    "#,
                )
                .bind(*document_id)
                .bind(self.knowledge_base_id)
                .bind(self.deployment_id)
                .bind(*chunk_index as i32)
                .bind(chunk_content)
                .bind(embedding_vector)
                .bind(now)
                .bind(now)
                .execute(&app_state.db_pool)
                .await {
                    Ok(_) => {
                        total_stored_chunks += 1;
                    }
                    Err(e) => {
                        error!(
                            "Failed to store chunk {} for document {}: {}",
                            chunk_index, document_id, e
                        );
                    }
                }
            }
        }

        // Update all successfully processed documents to completed status
        for (document_id, chunk_count) in document_chunk_counts {
            let stored_count = all_chunks.iter().filter(|(id, _, _)| *id == document_id).count();
            
            let _ = sqlx::query!(
                r#"
                UPDATE ai_knowledge_base_documents 
                SET processing_metadata = jsonb_set(
                    jsonb_set(
                        jsonb_set(
                            COALESCE(processing_metadata, '{}'),
                            '{status}',
                            '"completed"'
                        ),
                        '{chunks_count}',
                        $1::text::jsonb
                    ),
                    '{processed_at}',
                    $2::text::jsonb
                ),
                updated_at = $3
                WHERE id = $4
                "#,
                stored_count.to_string(),
                Utc::now().to_rfc3339(),
                Utc::now(),
                document_id
            )
            .execute(&app_state.db_pool)
            .await;
        }

        Ok(format!(
            "Processed {} of {} documents with {} total chunks",
            processed_count,
            pending_documents_len,
            total_stored_chunks
        ))
    }
}

impl ProcessDocumentBatchCommand {
    async fn extract_document_chunks(
        &self,
        document_id: i64,
        file_url: &str,
        file_type: &str,
        app_state: &AppState,
    ) -> Result<Vec<String>, AppError> {
        // Download file content from S3
        info!("Downloading file from: {}", file_url);
        
        // Parse S3 URL to get bucket and key
        let url_parts: Vec<&str> = file_url.strip_prefix("https://")
            .or_else(|| file_url.strip_prefix("http://"))
            .unwrap_or(file_url)
            .split('/')
            .collect();
        
        if url_parts.len() < 2 {
            return Err(AppError::BadRequest(format!("Invalid S3 URL format: {}", file_url)));
        }
        
        let bucket_and_domain = url_parts[0];
        let bucket = bucket_and_domain.split('.').next()
            .ok_or_else(|| AppError::BadRequest(format!("Could not extract bucket from URL: {}", file_url)))?;
        let key = url_parts[1..].join("/");
        
        let response = app_state.s3_client
            .get_object()
            .bucket(bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to get object from S3: {}", e)))?;

        let file_content = response
            .body
            .collect()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to read S3 object body: {}", e)))?
            .into_bytes();

        // Extract and process text
        let text = app_state
            .text_processing_service
            .extract_text_from_file(&file_content, file_type)?;
        
        let cleaned_text = app_state.text_processing_service.clean_text(&text);

        // Use larger chunks (2000 chars) with reduced overlap (200 chars)
        // This provides better context for embeddings
        let chunks = app_state
            .text_processing_service
            .chunk_text(&cleaned_text, 2000, 200)?;

        // Return chunk texts
        let chunk_texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
        Ok(chunk_texts)
    }
}