use crate::{
    error::AppError, state::AppState,
    commands::Command,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteWorkspaceCommand {
    pub deployment_id: i64,
    pub workspace_id: i64,
}

impl DeleteWorkspaceCommand {
    pub fn new(deployment_id: i64, workspace_id: i64) -> Self {
        Self {
            deployment_id,
            workspace_id,
        }
    }
}

impl Command for DeleteWorkspaceCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut tx = app_state.db_pool.begin().await
            .map_err(|e| AppError::Database(e))?;

        // First check if workspace exists and belongs to deployment
        let exists = sqlx::query!(
            "SELECT id FROM workspaces WHERE id = $1 AND organization_id IN (SELECT id FROM organizations WHERE deployment_id = $2)",
            self.workspace_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        if exists.is_none() {
            return Err(AppError::NotFound("Workspace not found".to_string()));
        }

        // Delete workspace memberships for this workspace
        sqlx::query!(
            "DELETE FROM workspace_memberships WHERE workspace_id = $1",
            self.workspace_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Delete workspace roles for this workspace
        sqlx::query!(
            "DELETE FROM workspace_roles WHERE workspace_id = $1",
            self.workspace_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Finally delete the workspace
        sqlx::query!(
            "DELETE FROM workspaces WHERE id = $1",
            self.workspace_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        tx.commit().await
            .map_err(|e| AppError::Database(e))?;

        Ok(())
    }
}
