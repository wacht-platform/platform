use common::error::AppError;

pub struct UpdateApiKeyLastUsedCommand {
    pub key_id: i64,
}

impl UpdateApiKeyLastUsedCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        sqlx::query!(
            r#"
            UPDATE api_keys
            SET last_used_at = NOW()
            WHERE id = $1
            "#,
            self.key_id
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}
