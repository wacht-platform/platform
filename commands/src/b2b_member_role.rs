use common::error::AppError;

pub struct AddOrganizationMemberRoleCommand {
    pub deployment_id: i64,
    pub organization_id: i64,
    pub membership_id: i64,
    pub role_id: i64,
}

impl AddOrganizationMemberRoleCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            WITH org AS (
                SELECT id FROM organizations
                WHERE id = $2 AND deployment_id = $1 AND deleted_at IS NULL
            ),
            membership AS (
                SELECT id FROM organization_memberships
                WHERE id = $3
                  AND organization_id = $2
                  AND deleted_at IS NULL
                  AND EXISTS(SELECT 1 FROM org)
            ),
            role AS (
                SELECT id FROM organization_roles
                WHERE id = $4
                  AND deployment_id = $1
                  AND (organization_id IS NULL OR organization_id = $2)
                  AND EXISTS(SELECT 1 FROM org)
            ),
            inserted AS (
                INSERT INTO organization_membership_roles (
                    organization_membership_id, organization_role_id, organization_id
                )
                SELECT $3, $4, $2
                WHERE EXISTS(SELECT 1 FROM membership) AND EXISTS(SELECT 1 FROM role)
                ON CONFLICT (organization_membership_id, organization_role_id) DO NOTHING
            )
            SELECT
                EXISTS(SELECT 1 FROM membership) AS "membership_exists!",
                EXISTS(SELECT 1 FROM role) AS "role_exists!"
            "#,
            self.deployment_id,
            self.organization_id,
            self.membership_id,
            self.role_id,
        )
        .fetch_one(executor)
        .await?;

        if !result.membership_exists {
            return Err(AppError::NotFound(
                "organization membership not found".to_string(),
            ));
        }
        if !result.role_exists {
            return Err(AppError::NotFound(
                "role not found in this organization".to_string(),
            ));
        }
        Ok(())
    }
}

pub struct RemoveOrganizationMemberRoleCommand {
    pub deployment_id: i64,
    pub organization_id: i64,
    pub membership_id: i64,
    pub role_id: i64,
}

impl RemoveOrganizationMemberRoleCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            WITH org AS (
                SELECT id FROM organizations
                WHERE id = $2 AND deployment_id = $1 AND deleted_at IS NULL
            ),
            membership AS (
                SELECT id FROM organization_memberships
                WHERE id = $3
                  AND organization_id = $2
                  AND deleted_at IS NULL
                  AND EXISTS(SELECT 1 FROM org)
            )
            DELETE FROM organization_membership_roles
            WHERE organization_membership_id = $3
              AND organization_role_id = $4
              AND organization_id = $2
              AND EXISTS(SELECT 1 FROM membership)
            RETURNING organization_membership_id
            "#,
            self.deployment_id,
            self.organization_id,
            self.membership_id,
            self.role_id,
        )
        .fetch_optional(executor)
        .await?;

        // We can't distinguish "membership doesn't exist" from "role wasn't on
        // the member" with this single statement, so do a second cheap check.
        if result.is_none() {
            // Either membership missing or role wasn't attached. Treat both as
            // 404 since the caller likely wants to know neither state holds.
            return Err(AppError::NotFound(
                "role not assigned to this membership".to_string(),
            ));
        }
        Ok(())
    }
}

pub struct AddWorkspaceMemberRoleCommand {
    pub deployment_id: i64,
    pub workspace_id: i64,
    pub membership_id: i64,
    pub role_id: i64,
}

impl AddWorkspaceMemberRoleCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            WITH ws AS (
                SELECT id, organization_id FROM workspaces
                WHERE id = $2 AND deployment_id = $1 AND deleted_at IS NULL
            ),
            membership AS (
                SELECT wm.id, wm.organization_id
                FROM workspace_memberships wm
                JOIN ws ON ws.id = wm.workspace_id
                WHERE wm.id = $3
                  AND wm.workspace_id = $2
                  AND wm.deleted_at IS NULL
            ),
            role AS (
                SELECT id FROM workspace_roles
                WHERE id = $4
                  AND deployment_id = $1
                  AND (workspace_id IS NULL OR workspace_id = $2)
                  AND EXISTS(SELECT 1 FROM ws)
            ),
            inserted AS (
                INSERT INTO workspace_membership_roles (
                    workspace_membership_id, workspace_role_id, workspace_id, organization_id
                )
                SELECT $3, $4, $2, (SELECT organization_id FROM membership)
                WHERE EXISTS(SELECT 1 FROM membership) AND EXISTS(SELECT 1 FROM role)
                ON CONFLICT (workspace_membership_id, workspace_role_id) DO NOTHING
            )
            SELECT
                EXISTS(SELECT 1 FROM membership) AS "membership_exists!",
                EXISTS(SELECT 1 FROM role) AS "role_exists!"
            "#,
            self.deployment_id,
            self.workspace_id,
            self.membership_id,
            self.role_id,
        )
        .fetch_one(executor)
        .await?;

        if !result.membership_exists {
            return Err(AppError::NotFound(
                "workspace membership not found".to_string(),
            ));
        }
        if !result.role_exists {
            return Err(AppError::NotFound(
                "role not found in this workspace".to_string(),
            ));
        }
        Ok(())
    }
}

pub struct RemoveWorkspaceMemberRoleCommand {
    pub deployment_id: i64,
    pub workspace_id: i64,
    pub membership_id: i64,
    pub role_id: i64,
}

impl RemoveWorkspaceMemberRoleCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            WITH ws AS (
                SELECT id FROM workspaces
                WHERE id = $2 AND deployment_id = $1 AND deleted_at IS NULL
            ),
            membership AS (
                SELECT id FROM workspace_memberships
                WHERE id = $3
                  AND workspace_id = $2
                  AND deleted_at IS NULL
                  AND EXISTS(SELECT 1 FROM ws)
            )
            DELETE FROM workspace_membership_roles
            WHERE workspace_membership_id = $3
              AND workspace_role_id = $4
              AND workspace_id = $2
              AND EXISTS(SELECT 1 FROM membership)
            RETURNING workspace_membership_id
            "#,
            self.deployment_id,
            self.workspace_id,
            self.membership_id,
            self.role_id,
        )
        .fetch_optional(executor)
        .await?;

        if result.is_none() {
            return Err(AppError::NotFound(
                "role not assigned to this membership".to_string(),
            ));
        }
        Ok(())
    }
}
