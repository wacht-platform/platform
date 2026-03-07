use common::error::AppError;
use serde::{Deserialize, Serialize};

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

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            WITH target_workspace AS (
                SELECT id
                FROM workspaces
                WHERE id = $1
                  AND organization_id IN (
                      SELECT id FROM organizations WHERE deployment_id = $2
                  )
            ),
            deleted_memberships AS (
                DELETE FROM workspace_memberships
                WHERE workspace_id IN (SELECT id FROM target_workspace)
            ),
            deleted_roles AS (
                DELETE FROM workspace_roles
                WHERE workspace_id IN (SELECT id FROM target_workspace)
            ),
            deleted_workspace AS (
                DELETE FROM workspaces
                WHERE id IN (SELECT id FROM target_workspace)
            )
            SELECT EXISTS(SELECT 1 FROM target_workspace) AS "workspace_exists!"
            "#,
            self.workspace_id,
            self.deployment_id
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        if !result.workspace_exists {
            return Err(AppError::NotFound("Workspace not found".to_string()));
        }

        Ok(())
    }
}
