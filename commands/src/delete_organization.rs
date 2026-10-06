use common::error::AppError;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct DeleteOrganizationCommand {
    pub deployment_id: i64,
    pub organization_id: i64,
}

#[cfg(test)]
mod tests {
    #[test]
    fn organization_children_are_gated_by_scoped_parent() {
        let source = include_str!("delete_organization.rs");
        let sql = source
            .split("r#\"")
            .nth(1)
            .unwrap()
            .split("\"#")
            .next()
            .unwrap();
        assert!(sql.contains("WHERE deployment_id = $1 AND id = $2"));
        assert!(!sql.contains("organization_id = $2"));
        assert_eq!(
            sql.matches("organization_id IN (SELECT id FROM org)")
                .count(),
            9
        );
        assert!(sql.contains(
            "DELETE FROM organizations\n                WHERE id IN (SELECT id FROM org)"
        ));
    }
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
        let org_exists: bool = sqlx::query_scalar(
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
                    WHERE organization_id IN (SELECT id FROM org)
                )
            ),
            deleted_workspace_membership_roles AS (
                DELETE FROM workspace_membership_roles
                WHERE workspace_membership_id IN (
                    SELECT id
                    FROM workspace_memberships
                    WHERE organization_id IN (SELECT id FROM org)
                )
            ),
            deleted_workspace_memberships AS (
                DELETE FROM workspace_memberships
                WHERE organization_id IN (SELECT id FROM org)
            ),
            deleted_workspace_roles AS (
                DELETE FROM workspace_roles
                WHERE organization_id IN (SELECT id FROM org)
            ),
            deleted_workspaces AS (
                DELETE FROM workspaces
                WHERE organization_id IN (SELECT id FROM org)
            ),
            updated_signins_org AS (
                UPDATE signins
                SET active_organization_membership_id = NULL
                WHERE active_organization_membership_id IN (
                    SELECT id
                    FROM organization_memberships
                    WHERE organization_id IN (SELECT id FROM org)
                )
            ),
            deleted_org_membership_roles AS (
                DELETE FROM organization_membership_roles
                WHERE organization_id IN (SELECT id FROM org)
            ),
            deleted_org_memberships AS (
                DELETE FROM organization_memberships
                WHERE organization_id IN (SELECT id FROM org)
            ),
            deleted_org_roles AS (
                DELETE FROM organization_roles
                WHERE organization_id IN (SELECT id FROM org)
            ),
            deleted_org AS (
                DELETE FROM organizations
                WHERE id IN (SELECT id FROM org)
            )
            SELECT EXISTS(SELECT 1 FROM org)
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.organization_id)
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        if !org_exists {
            return Err(AppError::NotFound("Organization not found".to_string()));
        }

        Ok(())
    }
}
