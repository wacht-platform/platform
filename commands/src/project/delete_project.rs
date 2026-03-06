use super::*;
use sqlx::Connection;
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

    pub async fn run_with_tx(
        self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), AppError> {
        let deployment_ids = ActiveDeploymentIdsByProjectQuery::builder()
            .project_id(self.id)
            .execute_with(tx.as_mut())
            .await?;

        DeleteDeploymentSocialConnectionsByIds::builder()
            .deployment_ids(deployment_ids.clone())
            .execute_with(tx.as_mut())
            .await?;

        DeleteDeploymentAuthSettingsByIds::builder()
            .deployment_ids(deployment_ids.clone())
            .execute_with(tx.as_mut())
            .await?;

        DeleteDeploymentUiSettingsByIds::builder()
            .deployment_ids(deployment_ids.clone())
            .execute_with(tx.as_mut())
            .await?;

        DeleteDeploymentB2bSettingsByIds::builder()
            .deployment_ids(deployment_ids)
            .execute_with(tx.as_mut())
            .await?;

        DeleteDeploymentsByProject::builder()
            .project_id(self.id)
            .execute_with(tx.as_mut())
            .await?;

        DeleteProjectById::builder()
            .project_id(self.id)
            .execute_with(tx.as_mut())
            .await?;

        Ok(())
    }

    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let mut tx = conn.begin().await?;
        self.run_with_tx(&mut tx).await?;
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
