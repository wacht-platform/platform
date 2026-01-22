use common::{error::AppError, state::AppState};

pub struct GetDirtyStorageDeploymentsQuery;

impl crate::Query for GetDirtyStorageDeploymentsQuery {
    type Output = Vec<(i64, i64)>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let rows = sqlx::query!(
            "SELECT deployment_id, total_bytes 
             FROM deployment_storage_usage 
             WHERE is_dirty = true"
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        let deployments = rows
            .into_iter()
            .map(|row| (row.deployment_id, row.total_bytes))
            .collect();

        Ok(deployments)
    }
}
