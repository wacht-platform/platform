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

    pub async fn execute_with_db<'a, Db>(self, db: Db) -> Result<(), AppError>
    where
        Db: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = db.begin().await?;
        let deployment_ids = queries::ActiveDeploymentIdsByProjectQuery::builder()
            .project_id(self.id)
            .execute_with_db(tx.as_mut())
            .await?;

        DeleteDeploymentSocialConnectionsByIds::builder()
            .deployment_ids(deployment_ids.clone())
            .execute_with_db(tx.as_mut())
            .await?;

        DeleteDeploymentAuthSettingsByIds::builder()
            .deployment_ids(deployment_ids.clone())
            .execute_with_db(tx.as_mut())
            .await?;

        DeleteDeploymentUiSettingsByIds::builder()
            .deployment_ids(deployment_ids.clone())
            .execute_with_db(tx.as_mut())
            .await?;

        DeleteDeploymentB2bSettingsByIds::builder()
            .deployment_ids(deployment_ids)
            .execute_with_db(tx.as_mut())
            .await?;

        DeleteDeploymentsByProject::builder()
            .project_id(self.id)
            .execute_with_db(tx.as_mut())
            .await?;

        DeleteProjectById::builder()
            .project_id(self.id)
            .execute_with_db(tx.as_mut())
            .await?;

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
