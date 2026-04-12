use common::error::AppError;

pub struct RevokeApiKeyCommand {
    pub key_id: i64,
    pub deployment_id: i64,
    pub reason: Option<String>,
}

impl RevokeApiKeyCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            UPDATE api_keys
            SET
                is_active = false,
                revoked_at = NOW(),
                revoked_reason = $3,
                updated_at = NOW()
            WHERE id = $1 AND deployment_id = $2 AND is_active = true
            "#,
            self.key_id,
            self.deployment_id,
            self.reason
        )
        .execute(executor)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                "API key not found or already revoked".to_string(),
            ));
        }

        Ok(())
    }
}
