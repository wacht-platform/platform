use common::error::AppError;
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

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            WITH org AS (
                SELECT id
                FROM organizations
                WHERE deployment_id = $1 AND id = $2
            ),
            updated_signins_workspace AS (
                UPDATE signins
                SET active_workspace_membership_id = NULL
                WHERE active_workspace_membership_id IN (
                    SELECT id
                    FROM workspace_memberships
                    WHERE organization_id = $2
                )
            ),
            deleted_workspace_membership_roles AS (
                DELETE FROM workspace_membership_roles
                WHERE workspace_membership_id IN (
                    SELECT id
                    FROM workspace_memberships
                    WHERE organization_id = $2
                )
            ),
            deleted_workspace_memberships AS (
                DELETE FROM workspace_memberships
                WHERE organization_id = $2
            ),
            deleted_workspace_roles AS (
                DELETE FROM workspace_roles
                WHERE organization_id = $2
            ),
            deleted_workspaces AS (
                DELETE FROM workspaces
                WHERE organization_id = $2
            ),
            updated_signins_org AS (
                UPDATE signins
                SET active_organization_membership_id = NULL
                WHERE active_organization_membership_id IN (
                    SELECT id
                    FROM organization_memberships
                    WHERE organization_id = $2
                )
            ),
            deleted_org_membership_roles AS (
                DELETE FROM organization_membership_roles
                WHERE organization_id = $2
            ),
            deleted_org_memberships AS (
                DELETE FROM organization_memberships
                WHERE organization_id = $2
            ),
            deleted_org_roles AS (
                DELETE FROM organization_roles
                WHERE organization_id = $2
            ),
            deleted_org AS (
                DELETE FROM organizations
                WHERE deployment_id = $1 AND id = $2
            )
            SELECT EXISTS(SELECT 1 FROM org) AS "org_exists!"
            "#,
            self.deployment_id,
            self.organization_id
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        if !result.org_exists {
            return Err(AppError::NotFound("Organization not found".to_string()));
        }

        Ok(())
    }
}
