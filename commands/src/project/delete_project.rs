use super::*;
#[allow(dead_code)]
pub struct DeleteProjectCommand {
    id: i64,
}

impl DeleteProjectCommand {
    pub fn new(id: i64) -> Self {
        Self { id }
    }
}

impl Command for DeleteProjectCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut tx = app_state.db_pool.begin().await?;

        let deployment_ids = ActiveDeploymentIdsByProjectQuery::builder()
            .project_id(self.id)
            .execute_in_tx(&mut tx)
            .await?;

        DeleteDeploymentSocialConnectionsByIds::builder()
            .deployment_ids(deployment_ids.clone())
            .execute_in_tx(&mut tx)
            .await?;

        DeleteDeploymentAuthSettingsByIds::builder()
            .deployment_ids(deployment_ids.clone())
            .execute_in_tx(&mut tx)
            .await?;

        DeleteDeploymentUiSettingsByIds::builder()
            .deployment_ids(deployment_ids.clone())
            .execute_in_tx(&mut tx)
            .await?;

        DeleteDeploymentB2bSettingsByIds::builder()
            .deployment_ids(deployment_ids)
            .execute_in_tx(&mut tx)
            .await?;

        DeleteDeploymentsByProject::builder()
            .project_id(self.id)
            .execute_in_tx(&mut tx)
            .await?;

        DeleteProjectById::builder()
            .project_id(self.id)
            .execute_in_tx(&mut tx)
            .await?;

        tx.commit().await?;

        Ok(())
    }
}
