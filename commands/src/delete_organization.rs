use common::error::AppError;
use serde::{Deserialize, Serialize};
use sqlx::Connection;

#[derive(Serialize, Deserialize)]
pub struct DeleteOrganizationCommand {
    pub deployment_id: i64,
    pub organization_id: i64,
}

impl DeleteOrganizationCommand {
    pub fn new(deployment_id: i64, organization_id: i64) -> Self {
        Self {
            deployment_id,
            organization_id,
        }
    }

    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let mut tx = conn.begin().await.map_err(AppError::Database)?;

        let exists = sqlx::query!(
            "SELECT id FROM organizations WHERE deployment_id = $1 AND id = $2",
            self.deployment_id,
            self.organization_id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        if exists.is_none() {
            return Err(AppError::NotFound("Organization not found".to_string()));
        }

        sqlx::query!(
            "UPDATE signins SET active_workspace_membership_id = NULL WHERE active_workspace_membership_id IN (SELECT id FROM workspace_memberships WHERE organization_id = $1)",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        sqlx::query!(
            "DELETE FROM workspace_membership_roles WHERE workspace_membership_id IN (SELECT id FROM workspace_memberships WHERE organization_id = $1)",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        sqlx::query!(
            "DELETE FROM workspace_memberships WHERE organization_id = $1",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        sqlx::query!(
            "DELETE FROM workspace_roles WHERE organization_id = $1",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        sqlx::query!(
            "DELETE FROM workspaces WHERE organization_id = $1",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        sqlx::query!(
            "UPDATE signins SET active_organization_membership_id = NULL WHERE active_organization_membership_id IN (SELECT id FROM organization_memberships WHERE organization_id = $1)",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        sqlx::query!(
            "DELETE FROM organization_membership_roles WHERE organization_id = $1",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        sqlx::query!(
            "DELETE FROM organization_memberships WHERE organization_id = $1",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        sqlx::query!(
            "DELETE FROM organization_roles WHERE organization_id = $1",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        sqlx::query!(
            "DELETE FROM organizations WHERE deployment_id = $1 AND id = $2",
            self.deployment_id,
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        tx.commit().await.map_err(AppError::Database)?;

        Ok(())
    }
}
