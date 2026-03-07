use common::error::AppError;

pub struct GetDirtyStorageDeploymentsQuery;

impl GetDirtyStorageDeploymentsQuery {
    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<(i64, i64)>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            "SELECT deployment_id, total_bytes 
             FROM deployment_storage_usage 
             WHERE is_dirty = true"
        )
        .fetch_all(executor)
        .await?;

        let deployments = rows
            .into_iter()
            .map(|row| (row.deployment_id, row.total_bytes))
            .collect();

        Ok(deployments)
    }
}
