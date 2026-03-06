use common::{error::AppError, state::AppState};

pub struct GetDirtyStorageDeploymentsQuery;

impl GetDirtyStorageDeploymentsQuery {
    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<Vec<(i64, i64)>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rows = sqlx::query!(
            "SELECT deployment_id, total_bytes 
             FROM deployment_storage_usage 
             WHERE is_dirty = true"
        )
        .fetch_all(&mut *conn)
        .await?;

        let deployments = rows
            .into_iter()
            .map(|row| (row.deployment_id, row.total_bytes))
            .collect();

        Ok(deployments)
    }
}

impl crate::Query for GetDirtyStorageDeploymentsQuery {
    type Output = Vec<(i64, i64)>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}
