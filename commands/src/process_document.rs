use chrono::Utc;
use common::error::AppError;
use std::future::Future;

pub struct ProcessDocumentCommand {
    pub deployment_id: i64,
    pub knowledge_base_id: i64,
    pub document_id: i64,
}

pub struct ProcessDocumentDeps<'a, A, DispatchFn> {
    pub acquirer: A,
    pub storage_client: &'a aws_sdk_s3::Client,
    pub text_processing_service: &'a common::TextProcessingService,
    pub dispatch_batch: DispatchFn,
}

impl ProcessDocumentCommand {
    pub fn new(deployment_id: i64, knowledge_base_id: i64, document_id: i64) -> Self {
        Self {
            deployment_id,
            knowledge_base_id,
            document_id,
        }
    }

    pub async fn execute_with_deps<'a, A, DispatchFn, DispatchFut>(
        self,
        deps: ProcessDocumentDeps<'a, A, DispatchFn>,
    ) -> Result<String, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
        DispatchFn: Fn(i64, i64, usize) -> DispatchFut,
        DispatchFut: Future<Output = Result<(), AppError>>,
    {
        let mut tx = deps.acquirer.begin().await?;
        let now = Utc::now();

        let document = sqlx::query!(
            r#"
            SELECT id, file_url, file_type, title
            FROM ai_knowledge_base_documents 
            WHERE id = $1 AND knowledge_base_id = $2
            "#,
            self.document_id,
            self.knowledge_base_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        let response = deps
            .storage_client
            .get_object()
            .bucket("wacht-agents")
            .key(&document.file_url)
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

        let text =
            deps.text_processing_service
                .extract_text_from_file(&file_content, &document.file_type)?;
        let cleaned_text = deps.text_processing_service.clean_text(&text);
        let chunks = deps
            .text_processing_service
            .chunk_text(&cleaned_text, 2000, 200)?;

        if chunks.is_empty() {
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
                    '"No chunks were created"'::jsonb
                ),
                updated_at = $1
                WHERE id = $2
                "#,
                now,
                document.id
            )
            .execute(&mut *tx)
            .await;
            tx.commit().await.map_err(AppError::Database)?;
            return Ok("No chunks created from document".to_string());
        }

        for (chunk_index, chunk) in chunks.iter().enumerate() {
            sqlx::query!(
                r#"
                INSERT INTO knowledge_base_document_chunks
                (document_id, knowledge_base_id, deployment_id, chunk_index, content, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                "#,
                document.id,
                self.knowledge_base_id,
                self.deployment_id,
                chunk_index as i32,
                chunk.content,
                now,
                now
            )
            .execute(&mut *tx)
            .await
            .map_err(AppError::Database)?;
        }

        let _ = sqlx::query!(
            r#"
            UPDATE ai_knowledge_base_documents 
            SET processing_metadata = jsonb_set(
                jsonb_set(
                    COALESCE(processing_metadata, '{}'),
                    '{status}',
                    '"chunks_ready"'
                ),
                '{chunks_count}',
                $1::text::jsonb
            ),
            updated_at = $2
            WHERE id = $3
            "#,
            chunks.len().to_string(),
            now,
            document.id
        )
        .execute(&mut *tx)
        .await;

        if let Err(e) =
            (deps.dispatch_batch)(self.deployment_id, self.knowledge_base_id, 100).await
        {
            tracing::error!("Failed to dispatch embedding processing task: {}", e);
            let _ = sqlx::query!(
                r#"
                UPDATE ai_knowledge_base_documents 
                SET processing_metadata = jsonb_set(
                    COALESCE(processing_metadata, '{}'),
                    '{status}',
                    '"failed"'
                ),
                updated_at = $1
                WHERE id = $2
                "#,
                now,
                document.id
            )
            .execute(&mut *tx)
            .await;
        }

        tx.commit().await.map_err(AppError::Database)?;
        Ok(format!(
            "Processed document {} into {} chunks",
            document.title,
            chunks.len()
        ))
    }
}
