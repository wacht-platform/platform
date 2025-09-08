use crate::Command;
use common::error::AppError;
use common::state::AppState;
use serde::{Deserialize, Serialize};

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
}

impl Command for DeleteOrganizationCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut tx = app_state
            .db_pool
            .begin()
            .await
            .map_err(|e| AppError::Database(e))?;

        // First check if organization exists and belongs to deployment
        let exists = sqlx::query!(
            "SELECT id FROM organizations WHERE deployment_id = $1 AND id = $2",
            self.deployment_id,
            self.organization_id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        if exists.is_none() {
            return Err(AppError::NotFound("Organization not found".to_string()));
        }

        // Clear references to workspace memberships in signins table
        sqlx::query!(
            "UPDATE signins SET active_workspace_membership_id = NULL WHERE active_workspace_membership_id IN (SELECT id FROM workspace_memberships WHERE organization_id = $1)",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Delete workspace membership roles first (junction table)
        sqlx::query!(
            "DELETE FROM workspace_membership_roles WHERE workspace_membership_id IN (SELECT id FROM workspace_memberships WHERE organization_id = $1)",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Delete workspace memberships for this organization
        sqlx::query!(
            "DELETE FROM workspace_memberships WHERE organization_id = $1",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Delete workspace roles for this organization (workspace_roles has organization_id column)
        sqlx::query!(
            "DELETE FROM workspace_roles WHERE organization_id = $1",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Delete all workspaces for this organization
        sqlx::query!(
            "DELETE FROM workspaces WHERE organization_id = $1",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Clear references to organization memberships in signins table
        sqlx::query!(
            "UPDATE signins SET active_organization_membership_id = NULL WHERE active_organization_membership_id IN (SELECT id FROM organization_memberships WHERE organization_id = $1)",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Delete organization membership roles
        sqlx::query!(
            "DELETE FROM organization_membership_roles WHERE organization_id = $1",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Delete organization memberships
        sqlx::query!(
            "DELETE FROM organization_memberships WHERE organization_id = $1",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Delete organization roles
        sqlx::query!(
            "DELETE FROM organization_roles WHERE organization_id = $1",
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Finally delete the organization
        sqlx::query!(
            "DELETE FROM organizations WHERE deployment_id = $1 AND id = $2",
            self.deployment_id,
            self.organization_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        tx.commit().await.map_err(|e| AppError::Database(e))?;

        Ok(())
    }
}
