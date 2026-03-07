use common::error::AppError;

pub struct MarkStorageAsCleanCommand {
    pub deployment_id: i64,
}

impl MarkStorageAsCleanCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            "UPDATE deployment_storage_usage SET is_dirty = false WHERE deployment_id = $1",
            self.deployment_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}
