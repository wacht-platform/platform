use common::error::AppError;

pub struct MarkKnowledgeBaseDocumentFailedCommand {
    document_id: i64,
    error: Option<String>,
}

impl MarkKnowledgeBaseDocumentFailedCommand {
    pub fn new(document_id: i64) -> Self {
        Self {
            document_id,
            error: None,
        }
    }

    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(
            r#"
            UPDATE ai_knowledge_base_documents
            SET processing_metadata = CASE
                WHEN $1::text IS NULL THEN jsonb_set(
                    COALESCE(processing_metadata, '{}'),
                    '{status}',
                    '"failed"'
                )
                ELSE jsonb_set(
                    jsonb_set(
                        COALESCE(processing_metadata, '{}'),
                        '{status}',
                        '"failed"'
                    ),
                    '{error}',
                    to_jsonb($1::text)
                )
            END,
            updated_at = $2
            WHERE id = $3
            "#,
        )
        .bind(self.error)
        .bind(chrono::Utc::now())
        .bind(self.document_id)
        .execute(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(())
    }
}
