use common::error::AppError;

pub struct MarkStorageAsCleanCommand {
    pub deployment_id: i64,
}

impl MarkStorageAsCleanCommand {
    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        sqlx::query!(
            "UPDATE deployment_storage_usage SET is_dirty = false WHERE deployment_id = $1",
            self.deployment_id
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}
