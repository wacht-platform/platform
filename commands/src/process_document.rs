use crate::{Command, DispatchDocumentBatchTaskCommand};
use chrono::Utc;
use common::error::AppError;
use common::state::AppState;

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
}

impl Command for ProcessDocumentCommand {
    type Output = String;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        let file_key = document.file_url.clone();

        let storage_client = app_state
            .agent_storage_client
            .as_ref()
            .ok_or_else(|| AppError::Internal("Agent storage client not configured".to_string()))?;

        let response = storage_client
            .get_object()
            .bucket("wacht-agents")
            .key(&file_key)
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

        let text = app_state
            .text_processing_service
            .extract_text_from_file(&file_content, &document.file_type)?;

        let cleaned_text = app_state.text_processing_service.clean_text(&text);
        let chunks = app_state
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
            .execute(&app_state.db_pool)
            .await;

            return Ok("No chunks created from document".to_string());
        }

        let mut tx = app_state
            .db_pool
            .begin()
            .await
            .map_err(|e| AppError::Database(e))?;

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
            .map_err(|e| AppError::Database(e))?;
        }

        tx.commit().await.map_err(|e| AppError::Database(e))?;

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
        .execute(&app_state.db_pool)
        .await;

        let dispatch_task =
            DispatchDocumentBatchTaskCommand::new(self.deployment_id, self.knowledge_base_id, 100);

        if let Err(e) = dispatch_task.execute(app_state).await {
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
            .execute(&app_state.db_pool)
            .await;
        }

        Ok(format!(
            "Processed document {} into {} chunks",
            document.title,
            chunks.len()
        ))
    }
}
