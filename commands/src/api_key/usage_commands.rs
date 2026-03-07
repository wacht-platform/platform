use common::error::AppError;

pub struct UpdateApiKeyLastUsedCommand {
    pub key_id: i64,
}

impl UpdateApiKeyLastUsedCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE api_keys
            SET last_used_at = NOW()
            WHERE id = $1
            "#,
            self.key_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}
