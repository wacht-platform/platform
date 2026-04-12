use common::error::AppError;

pub struct DeleteApiAuthAppCommand {
    pub app_slug: String,
    pub deployment_id: i64,
}

impl DeleteApiAuthAppCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            UPDATE api_auth_apps
            SET deleted_at = NOW()
            WHERE app_slug = $1 AND deployment_id = $2 AND deleted_at IS NULL
            "#,
            self.app_slug,
            self.deployment_id
        )
        .execute(executor)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("API auth app not found".to_string()));
        }

        Ok(())
    }
}
