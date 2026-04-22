use crate::{
    DispatchVectorStoreMaintenanceTaskCommand, GenerateEmbeddingsCommand,
    ResolveDeploymentStorageCommand, VECTOR_STORE_KNOWLEDGE_BASE,
    ai_knowledge_base_document_status::MarkKnowledgeBaseDocumentFailedCommand,
    build_multimodal_retrieval_document_parts, resolve_deployment_embedding_dimension,
    resolve_deployment_embedding_settings,
};
use common::{
    EmbeddingApiProvider, EmbeddingPart, HasDbRouter, HasEmbeddingProvider, HasEncryptionProvider,
    HasNatsProvider, HasTextProcessingProvider, KnowledgeBaseChunkRecord, error::AppError,
    replace_document_chunks,
};
const KNOWLEDGE_CHUNK_SIZE_TOKENS: usize = 5_000;
const KNOWLEDGE_CHUNK_OVERLAP_TOKENS: usize = 500;
const PDF_PAGES_PER_EMBEDDING_CHUNK: usize = 6;

pub struct ProcessDocumentCommand {
    pub deployment_id: i64,
    pub knowledge_base_id: i64,
    pub document_id: i64,
}

impl ProcessDocumentCommand {
    pub fn new(deployment_id: i64, knowledge_base_id: i64, document_id: i64) -> Self {
        Self {
            deployment_id,
            knowledge_base_id,
            document_id,
        }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<String, AppError>
    where
        D: HasDbRouter
            + HasEncryptionProvider
            + HasEmbeddingProvider
            + HasNatsProvider
            + HasTextProcessingProvider
            + ?Sized,
    {
        let document = sqlx::query!(
            r#"
            SELECT id, storage_object_key, file_type, title, description
            FROM ai_knowledge_base_documents 
            WHERE id = $1 AND knowledge_base_id = $2
            "#,
            self.document_id,
            self.knowledge_base_id
        )
        .fetch_one(deps.writer_pool())
        .await
        .map_err(AppError::Database)?;
        let document_id = document.id;
        let storage_object_key = document.storage_object_key;
        let file_type = document.file_type;
        let title = document.title;
        let description = document.description;

        let storage = ResolveDeploymentStorageCommand::new(self.deployment_id)
            .execute_with_deps(deps)
            .await?;

        let response = storage
            .client()
            .get_object()
            .bucket(storage.bucket())
            .key(&storage_object_key)
            .send()
            .await
            .map_err(|e| {
                AppError::Internal(format!("Failed to download file from storage: {}", e))
            })?;

        let file_content = response
            .body
            .collect()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to read file content: {}", e)))?
            .into_bytes()
            .to_vec();

        let lance_config = storage.vector_store_config();
        if !storage.vector_store_initialized {
            return Err(AppError::Validation(
                "Deployment vector store is not initialized. Re-save AI storage settings first."
                    .to_string(),
            ));
        }

        let lance_rows = if file_type.eq_ignore_ascii_case("application/pdf")
            || file_type.eq_ignore_ascii_case("pdf")
        {
            let pdf_chunks = deps
                .text_processing_provider()
                .split_pdf_into_page_groups(&file_content, PDF_PAGES_PER_EMBEDDING_CHUNK)?;

            if pdf_chunks.is_empty() {
                let _ = MarkKnowledgeBaseDocumentFailedCommand::new(document_id)
                    .with_error("No PDF page groups were created")
                    .execute_with_db(deps.writer_pool())
                    .await;
                return Err(AppError::Internal(
                    "No PDF page groups were created from document".to_string(),
                ));
            }

            let embedding_settings =
                resolve_deployment_embedding_settings(deps, self.deployment_id).await?;
            let mut rows = Vec::with_capacity(pdf_chunks.len());
            for pdf_chunk in pdf_chunks {
                let content = format!(
                    "title: {} | pages: {}-{}",
                    title, pdf_chunk.start_page, pdf_chunk.end_page
                );
                let provider = match embedding_settings.provider {
                    models::DeploymentEmbeddingProvider::Gemini => EmbeddingApiProvider::Gemini,
                    models::DeploymentEmbeddingProvider::Openai => EmbeddingApiProvider::Openai,
                    models::DeploymentEmbeddingProvider::Openrouter => {
                        EmbeddingApiProvider::Openrouter
                    }
                };
                let parts = if matches!(provider, EmbeddingApiProvider::Gemini) {
                    build_multimodal_retrieval_document_parts(
                        &embedding_settings.model,
                        &content,
                        Some(&title),
                        "application/pdf",
                        pdf_chunk.content,
                    )
                } else {
                    let extracted_text = deps
                        .text_processing_provider()
                        .extract_text_from_file(&pdf_chunk.content, "application/pdf")?;
                    vec![EmbeddingPart::Text(extracted_text)]
                };
                let embedding = match deps
                    .embedding_provider()
                    .embed_parts_with(
                        provider,
                        &embedding_settings.model,
                        parts,
                        Some(embedding_settings.dimension),
                        Some(embedding_settings.api_key.as_str()),
                    )
                    .await
                {
                    Ok(embedding) => embedding,
                    Err(e) => {
                        let _ = MarkKnowledgeBaseDocumentFailedCommand::new(document_id)
                            .with_error(format!("Failed to generate PDF embeddings: {}", e))
                            .execute_with_db(deps.writer_pool())
                            .await;
                        return Err(e);
                    }
                };

                rows.push(KnowledgeBaseChunkRecord {
                    knowledge_base_id: self.knowledge_base_id,
                    document_id,
                    path: storage_object_key.clone(),
                    title: title.clone(),
                    description: description.clone(),
                    content,
                    embedding: Some(embedding),
                });
            }
            rows
        } else {
            let text = deps
                .text_processing_provider()
                .extract_text_from_file(&file_content, &file_type)?;
            let cleaned_text = deps.text_processing_provider().clean_text(&text);
            let chunks = deps.text_processing_provider().chunk_text(
                &cleaned_text,
                KNOWLEDGE_CHUNK_SIZE_TOKENS,
                KNOWLEDGE_CHUNK_OVERLAP_TOKENS,
            )?;

            if chunks.is_empty() {
                let _ = MarkKnowledgeBaseDocumentFailedCommand::new(document_id)
                    .with_error("No chunks were created")
                    .execute_with_db(deps.writer_pool())
                    .await;
                return Err(AppError::Internal(
                    "No chunks were created from document".to_string(),
                ));
            }

            let chunk_texts = chunks
                .iter()
                .map(|chunk| chunk.content.clone())
                .collect::<Vec<_>>();
            let embeddings = match GenerateEmbeddingsCommand::new(chunk_texts)
                .with_titles(vec![Some(title.clone()); chunks.len()])
                .for_retrieval_document()
                .for_deployment(self.deployment_id)
                .execute_with_deps(deps)
                .await
            {
                Ok(embeddings) => embeddings,
                Err(e) => {
                    let _ = MarkKnowledgeBaseDocumentFailedCommand::new(document_id)
                        .with_error(format!("Failed to generate embeddings: {}", e))
                        .execute_with_db(deps.writer_pool())
                        .await;
                    return Err(e);
                }
            };

            chunks
                .iter()
                .zip(embeddings.into_iter())
                .map(|(chunk, embedding)| KnowledgeBaseChunkRecord {
                    knowledge_base_id: self.knowledge_base_id,
                    document_id,
                    path: storage_object_key.clone(),
                    title: title.clone(),
                    description: description.clone(),
                    content: chunk.content.clone(),
                    embedding: Some(embedding),
                })
                .collect::<Vec<_>>()
        };

        let embedding_dimension =
            resolve_deployment_embedding_dimension(deps, self.deployment_id).await?;
        replace_document_chunks(&lance_config, document_id, &lance_rows, embedding_dimension)
            .await?;

        DispatchVectorStoreMaintenanceTaskCommand::new(
            self.deployment_id,
            VECTOR_STORE_KNOWLEDGE_BASE.to_string(),
            format!("document-{}", document_id),
        )
        .execute_with_deps(deps)
        .await?;

        let _ = sqlx::query!(
            r#"
            UPDATE ai_knowledge_base_documents 
            SET processing_metadata = jsonb_set(
                jsonb_set(
                    COALESCE(processing_metadata, '{}'),
                    '{status}',
                        '"completed"'
                ),
                '{chunks_count}',
                $1::text::jsonb
            ),
            updated_at = $2
            WHERE id = $3
            "#,
            lance_rows.len().to_string(),
            chrono::Utc::now(),
            document_id
        )
        .execute(deps.writer_pool())
        .await;
        Ok(format!(
            "Processed document {} into {} chunks",
            title,
            lance_rows.len()
        ))
    }
}
