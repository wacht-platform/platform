use super::*;
#[allow(dead_code)]
pub struct DeleteProjectCommand {
    id: i64,
}

#[derive(Default)]
pub struct DeleteProjectCommandBuilder {
    id: Option<i64>,
}

impl DeleteProjectCommand {
    pub fn builder() -> DeleteProjectCommandBuilder {
        DeleteProjectCommandBuilder::default()
    }

    pub fn new(id: i64) -> Self {
        Self { id }
    }

    pub async fn execute_in_tx(
        self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), AppError> {
        let deployment_ids = ActiveDeploymentIdsByProjectQuery::builder()
            .project_id(self.id)
            .execute_in_tx(tx)
            .await?;

        DeleteDeploymentSocialConnectionsByIds::builder()
            .deployment_ids(deployment_ids.clone())
            .execute_in_tx(tx)
            .await?;

        DeleteDeploymentAuthSettingsByIds::builder()
            .deployment_ids(deployment_ids.clone())
            .execute_in_tx(tx)
            .await?;

        DeleteDeploymentUiSettingsByIds::builder()
            .deployment_ids(deployment_ids.clone())
            .execute_in_tx(tx)
            .await?;

        DeleteDeploymentB2bSettingsByIds::builder()
            .deployment_ids(deployment_ids)
            .execute_in_tx(tx)
            .await?;

        DeleteDeploymentsByProject::builder()
            .project_id(self.id)
            .execute_in_tx(tx)
            .await?;

        DeleteProjectById::builder()
            .project_id(self.id)
            .execute_in_tx(tx)
            .await?;

        Ok(())
    }

    pub async fn execute_with(self, writer: &sqlx::PgPool) -> Result<(), AppError> {
        let mut tx = writer.begin().await?;
        self.execute_in_tx(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }
}

impl DeleteProjectCommandBuilder {
    pub fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub fn build(self) -> Result<DeleteProjectCommand, AppError> {
        Ok(DeleteProjectCommand {
            id: self
                .id
                .ok_or_else(|| AppError::Validation("id is required".to_string()))?,
        })
    }
}

impl Command for DeleteProjectCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}
