use common::error::AppError;
use models::WorkspaceMemberDetails;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct AddWorkspaceMemberCommand {
    pub deployment_id: i64,
    pub workspace_id: i64,
    pub user_id: i64,
    pub role_ids: Vec<i64>,
}

impl AddWorkspaceMemberCommand {
    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        workspace_membership_id: i64,
        implicit_org_membership_id: i64,
    ) -> Result<WorkspaceMemberDetails, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        use sqlx::Connection;
        let mut conn = acquirer.acquire().await?;
        let mut tx = conn.begin().await?;

        let workspace = sqlx::query!(
            "SELECT id, organization_id FROM workspaces WHERE id = $1 AND deployment_id = $2",
            self.workspace_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        let workspace =
            workspace.ok_or_else(|| AppError::NotFound("Workspace not found".to_string()))?;

        // Check if user has an organization membership
        let org_membership = sqlx::query!(
            "SELECT id FROM organization_memberships WHERE user_id = $1 AND organization_id = $2",
            self.user_id,
            workspace.organization_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        let org_membership_id = if let Some(membership) = org_membership {
            membership.id
        } else {
            // User is not a member of the organization, create implicit membership
            // Get the deployment's default organization member role
            let default_org_role = sqlx::query!(
                r#"
                SELECT dbs.default_org_member_role_id
                FROM deployment_b2b_settings dbs
                WHERE dbs.deployment_id = $1
                "#,
                self.deployment_id
            )
            .fetch_optional(&mut *tx)
            .await?;

            let default_org_role_id = default_org_role
                .and_then(|r| Some(r.default_org_member_role_id))
                .ok_or_else(|| {
                    AppError::BadRequest(
                        "No default organization member role configured for this deployment"
                            .to_string(),
                    )
                })?;

            // Create organization membership
            let new_membership_id = implicit_org_membership_id;
            let now = chrono::Utc::now();

            sqlx::query!(
                r#"
                INSERT INTO organization_memberships (
                    id, created_at, updated_at, organization_id, user_id
                )
                VALUES ($1, $2, $3, $4, $5)
                "#,
                new_membership_id,
                now,
                now,
                workspace.organization_id,
                self.user_id
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| e)?;

            // Add the default role to the organization membership
            sqlx::query!(
                r#"
                INSERT INTO organization_membership_roles (organization_membership_id, organization_role_id, organization_id)
                VALUES ($1, $2, $3)
                "#,
                new_membership_id,
                default_org_role_id,
                workspace.organization_id
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                e
            })?;

            new_membership_id
        };

        // Check if membership already exists
        let existing = sqlx::query!(
            "SELECT id FROM workspace_memberships WHERE workspace_id = $1 AND user_id = $2",
            self.workspace_id,
            self.user_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        if existing.is_some() {
            return Err(AppError::BadRequest(
                "User is already a member of this workspace".to_string(),
            ));
        }

        // Create workspace membership
        let membership_id = workspace_membership_id;
        let now = chrono::Utc::now();

        sqlx::query!(
            r#"
            INSERT INTO workspace_memberships (
                id, created_at, updated_at, workspace_id, organization_id,
                organization_membership_id, user_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            membership_id,
            now,
            now,
            self.workspace_id,
            workspace.organization_id,
            org_membership_id,
            self.user_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| e)?;

        for (_, role_id) in self.role_ids.iter().enumerate() {
            sqlx::query!(
                r#"
                INSERT INTO workspace_membership_roles (workspace_membership_id, workspace_role_id, workspace_id, organization_id)
                VALUES ($1, $2, $3, $4)
                "#,
                membership_id,
                *role_id,
                self.workspace_id,
                workspace.organization_id
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                e
            })?;
        }

        tx.commit().await.map_err(|e| e)?;

        // Fetch the created member details (outside transaction)
        let member = sqlx::query!(
            r#"
            SELECT
                wm.id,
                wm.created_at,
                wm.updated_at,
                wm.user_id,
                wm.public_metadata,
                u.first_name,
                u.last_name,
                u.username,
                u.created_at as user_created_at,
                e.email_address as "primary_email_address?",
                p.phone_number as "primary_phone_number?"
            FROM workspace_memberships wm
            JOIN users u ON wm.user_id = u.id
            LEFT JOIN user_email_addresses e ON u.primary_email_address_id = e.id
            LEFT JOIN user_phone_numbers p ON u.primary_phone_number_id = p.id
            WHERE wm.id = $1
            "#,
            membership_id
        )
        .fetch_one(&mut *conn)
        .await?;

        // Get roles separately
        let role_rows = sqlx::query!(
            r#"
            SELECT wr.id, wr.name, wr.permissions
            FROM workspace_membership_roles wmr
            JOIN workspace_roles wr ON wmr.workspace_role_id = wr.id
            WHERE wmr.workspace_membership_id = $1
            "#,
            membership_id
        )
        .fetch_all(&mut *conn)
        .await?;

        let roles = role_rows
            .into_iter()
            .map(|r| models::WorkspaceRole {
                id: r.id,
                name: r.name,
                permissions: r.permissions,
                is_deployment_level: false,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .collect();

        let member_details = WorkspaceMemberDetails {
            id: member.id,
            created_at: member.created_at,
            updated_at: member.updated_at,
            workspace_id: self.workspace_id,
            user_id: member.user_id,
            public_metadata: member.public_metadata.clone(),
            first_name: member.first_name,
            last_name: member.last_name,
            username: if member.username.is_empty() {
                None
            } else {
                Some(member.username)
            },
            primary_email_address: member.primary_email_address,
            primary_phone_number: member.primary_phone_number,
            user_created_at: member.user_created_at,
            roles,
        };

        Ok(member_details)
    }
}

#[derive(Serialize, Deserialize)]
pub struct UpdateWorkspaceMemberCommand {
    pub deployment_id: i64,
    pub workspace_id: i64,
    pub membership_id: i64,
    pub role_ids: Option<Vec<i64>>,
    pub public_metadata: Option<serde_json::Value>,
}

impl UpdateWorkspaceMemberCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let membership = sqlx::query!(
            r#"
            SELECT wm.id, wm.organization_id
            FROM workspace_memberships wm
            JOIN workspaces w ON wm.workspace_id = w.id
            WHERE wm.id = $1 AND wm.workspace_id = $2 AND w.deployment_id = $3
            "#,
            self.membership_id,
            self.workspace_id,
            self.deployment_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        let membership = membership.ok_or(AppError::NotFound(
            "Workspace membership not found".to_string(),
        ))?;

        if let Some(role_ids) = self.role_ids {
            sqlx::query!(
                "DELETE FROM workspace_membership_roles WHERE workspace_membership_id = $1",
                self.membership_id
            )
            .execute(&mut *conn)
            .await?;

            for role_id in role_ids {
                sqlx::query!(
                    r#"
                INSERT INTO workspace_membership_roles (workspace_membership_id, workspace_role_id, workspace_id, organization_id)
                VALUES ($1, $2, $3, $4)
                "#,
                    self.membership_id,
                    role_id,
                    self.workspace_id,
                    membership.organization_id
                )
                .execute(&mut *conn)
                .await?;
            }
        }

        if let Some(metadata) = self.public_metadata {
            sqlx::query!(
                "UPDATE workspace_memberships SET public_metadata = $1, updated_at = NOW() WHERE id = $2",
                metadata,
                self.membership_id
            )
            .execute(&mut *conn)
            .await?;
        }

        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub struct RemoveWorkspaceMemberCommand {
    pub deployment_id: i64,
    pub workspace_id: i64,
    pub membership_id: i64,
}

impl RemoveWorkspaceMemberCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let membership = sqlx::query!(
            r#"
            SELECT wm.id
            FROM workspace_memberships wm
            JOIN workspaces w ON wm.workspace_id = w.id
            WHERE wm.id = $1 AND wm.workspace_id = $2 AND w.deployment_id = $3
            "#,
            self.membership_id,
            self.workspace_id,
            self.deployment_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        if membership.is_none() {
            return Err(AppError::NotFound(
                "Workspace membership not found".to_string(),
            ));
        }

        sqlx::query!(
            "UPDATE signins SET active_workspace_membership_id = NULL WHERE active_workspace_membership_id = $1",
            self.membership_id
        )
        .execute(&mut *conn)
        .await?;

        sqlx::query!(
            "DELETE FROM workspace_membership_roles WHERE workspace_membership_id = $1",
            self.membership_id
        )
        .execute(&mut *conn)
        .await?;

        sqlx::query!(
            "DELETE FROM workspace_memberships WHERE id = $1",
            self.membership_id
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}
