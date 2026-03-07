use common::error::AppError;
use serde::{Deserialize, Serialize};
use sqlx::Connection;

#[derive(Serialize, Deserialize)]
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

    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let mut tx = conn.begin().await.map_err(AppError::Database)?;

        let exists = sqlx::query!(
            "SELECT id FROM workspaces WHERE id = $1 AND organization_id IN (SELECT id FROM organizations WHERE deployment_id = $2)",
            self.workspace_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        if exists.is_none() {
            return Err(AppError::NotFound("Workspace not found".to_string()));
        }

        sqlx::query!(
            "DELETE FROM workspace_memberships WHERE workspace_id = $1",
            self.workspace_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        sqlx::query!(
            "DELETE FROM workspace_roles WHERE workspace_id = $1",
            self.workspace_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        sqlx::query!("DELETE FROM workspaces WHERE id = $1", self.workspace_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::Database)?;

        tx.commit().await.map_err(AppError::Database)?;

        Ok(())
    }
}
