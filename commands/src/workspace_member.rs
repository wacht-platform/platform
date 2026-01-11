use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::WorkspaceMemberDetails;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct AddWorkspaceMemberCommand {
    pub deployment_id: i64,
    pub workspace_id: i64,
    pub user_id: i64,
    pub role_ids: Vec<i64>,
}

impl Command for AddWorkspaceMemberCommand {
    type Output = WorkspaceMemberDetails;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        println!(
            "AddWorkspaceMemberCommand: Starting execution for workspace_id={}, user_id={}, role_ids={:?}",
            self.workspace_id, self.user_id, self.role_ids
        );

        let mut tx = app_state.db_pool.begin().await?;

        let workspace = sqlx::query!(
            "SELECT id, organization_id FROM workspaces WHERE id = $1 AND deployment_id = $2",
            self.workspace_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        let workspace = workspace.ok_or_else(|| {
            println!(
                "ERROR: Workspace not found: workspace_id={}, deployment_id={}",
                self.workspace_id, self.deployment_id
            );
            AppError::NotFound("Workspace not found".to_string())
        })?;

        println!(
            "Found workspace: id={}, organization_id={}",
            workspace.id, workspace.organization_id
        );

        // Check if user has an organization membership
        let org_membership = sqlx::query!(
            "SELECT id FROM organization_memberships WHERE user_id = $1 AND organization_id = $2",
            self.user_id,
            workspace.organization_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        let org_membership_id = if let Some(membership) = org_membership {
            println!(
                "User already has organization membership: membership_id={}",
                membership.id
            );
            membership.id
        } else {
            println!("User does not have organization membership, creating implicit membership");
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
                    println!("ERROR: No default organization member role configured for deployment_id={}", self.deployment_id);
                    AppError::BadRequest(
                        "No default organization member role configured for this deployment".to_string(),
                    )
                })?;

            println!("Found default org role: role_id={}", default_org_role_id);

            // Create organization membership
            let new_membership_id = app_state.sf.next_id()? as i64;
            let now = chrono::Utc::now();

            println!(
                "Creating organization membership: membership_id={}, user_id={}, org_id={}",
                new_membership_id, self.user_id, workspace.organization_id
            );

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
            .map_err(|e| {
                println!("ERROR: Failed to create organization membership: {:?}", e);
                e
            })?;

            println!("Created organization membership, now adding role");

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
                println!("ERROR: Failed to add role to organization membership: {:?}", e);
                e
            })?;

            println!("Successfully created implicit organization membership");
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
            println!(
                "WARN: User is already a member of workspace: workspace_id={}, user_id={}",
                self.workspace_id, self.user_id
            );
            return Err(AppError::BadRequest(
                "User is already a member of this workspace".to_string(),
            ));
        }

        // Create workspace membership
        let membership_id = app_state.sf.next_id()? as i64;
        let now = chrono::Utc::now();

        println!(
            "Creating workspace membership: membership_id={}, workspace_id={}, user_id={}, org_membership_id={}",
            membership_id, self.workspace_id, self.user_id, org_membership_id
        );

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
        .map_err(|e| {
            println!("ERROR: Failed to create workspace membership: {:?}", e);
            e
        })?;

        println!(
            "Created workspace membership, now adding {} roles",
            self.role_ids.len()
        );

        // Add roles
        for (idx, role_id) in self.role_ids.iter().enumerate() {
            println!(
                "Adding role {}/{}: role_id={}",
                idx + 1,
                self.role_ids.len(),
                role_id
            );
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
                println!("ERROR: Failed to add role {} to workspace membership: {:?}", role_id, e);
                e
            })?;
        }

        println!("Successfully added all roles to workspace membership");

        println!("Committing transaction");

        // Commit the transaction
        tx.commit().await.map_err(|e| {
            println!("ERROR: Failed to commit transaction: {:?}", e);
            e
        })?;

        println!("Transaction committed, fetching member details");

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
        .fetch_one(&app_state.db_pool)
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
        .fetch_all(&app_state.db_pool)
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

impl Command for UpdateWorkspaceMemberCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .fetch_optional(&app_state.db_pool)
        .await?;

        let membership = membership.ok_or(AppError::NotFound(
            "Workspace membership not found".to_string(),
        ))?;

        if let Some(role_ids) = self.role_ids {
            sqlx::query!(
                "DELETE FROM workspace_membership_roles WHERE workspace_membership_id = $1",
                self.membership_id
            )
            .execute(&app_state.db_pool)
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
                .execute(&app_state.db_pool)
                .await?;
            }
        }

        if let Some(metadata) = self.public_metadata {
            sqlx::query!(
                "UPDATE workspace_memberships SET public_metadata = $1, updated_at = NOW() WHERE id = $2",
                metadata,
                self.membership_id
            )
            .execute(&app_state.db_pool)
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

impl Command for RemoveWorkspaceMemberCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .fetch_optional(&app_state.db_pool)
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
        .execute(&app_state.db_pool)
        .await?;

        sqlx::query!(
            "DELETE FROM workspace_membership_roles WHERE workspace_membership_id = $1",
            self.membership_id
        )
        .execute(&app_state.db_pool)
        .await?;

        sqlx::query!(
            "DELETE FROM workspace_memberships WHERE id = $1",
            self.membership_id
        )
        .execute(&app_state.db_pool)
        .await?;

        Ok(())
    }
}
