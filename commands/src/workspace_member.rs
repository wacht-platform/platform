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

        // First check if workspace exists and belongs to deployment
        let workspace = sqlx::query!(
            "SELECT id, organization_id FROM workspaces WHERE id = $1 AND deployment_id = $2",
            self.workspace_id,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        let workspace = workspace.ok_or(AppError::NotFound("Workspace not found".to_string()))?;

        // Check if user has an organization membership
        let org_membership = sqlx::query!(
            "SELECT id FROM organization_memberships WHERE user_id = $1 AND organization_id = $2",
            self.user_id,
            workspace.organization_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        let org_membership = org_membership.ok_or(AppError::BadRequest(
            "User must be a member of the organization first".to_string()
        ))?;

        // Check if membership already exists
        let existing = sqlx::query!(
            "SELECT id FROM workspace_memberships WHERE workspace_id = $1 AND user_id = $2",
            self.workspace_id,
            self.user_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        if existing.is_some() {
            return Err(AppError::BadRequest(
                "User is already a member of this workspace".to_string(),
            ));
        }

        // Create workspace membership
        let membership_id = app_state.sf.next_id()? as i64;
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
            org_membership.id,
            self.user_id
        )
        .execute(&app_state.db_pool)
        .await?;

        // Add roles
        for role_id in &self.role_ids {
            sqlx::query!(
                r#"
                INSERT INTO workspace_membership_roles (workspace_membership_id, workspace_role_id)
                VALUES ($1, $2)
                "#,
                membership_id,
                *role_id
            )
            .execute(&app_state.db_pool)
            .await?;
        }

        // Update workspace member count
        sqlx::query!(
            "UPDATE workspaces SET member_count = member_count + 1 WHERE id = $1",
            self.workspace_id
        )
        .execute(&app_state.db_pool)
        .await?;

        // Fetch and return the full member details
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
    pub role_ids: Vec<i64>,
    pub public_metadata: Option<serde_json::Value>,
}


impl Command for UpdateWorkspaceMemberCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {

        // Verify membership exists and belongs to this workspace
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

        // Delete existing roles
        sqlx::query!(
            "DELETE FROM workspace_membership_roles WHERE workspace_membership_id = $1",
            self.membership_id
        )
        .execute(&app_state.db_pool)
        .await?;

        // Add new roles
        for role_id in &self.role_ids {
            sqlx::query!(
                r#"
                INSERT INTO workspace_membership_roles (workspace_membership_id, workspace_role_id)
                VALUES ($1, $2)
                "#,
                self.membership_id,
                *role_id
            )
            .execute(&app_state.db_pool)
            .await?;
        }

        // Update public_metadata if provided
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

        // Verify membership exists and belongs to this workspace
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

        // Clear workspace membership reference in signins
        sqlx::query!(
            "UPDATE signins SET active_workspace_membership_id = NULL WHERE active_workspace_membership_id = $1",
            self.membership_id
        )
        .execute(&app_state.db_pool)
        .await?;

        // Delete role associations
        sqlx::query!(
            "DELETE FROM workspace_membership_roles WHERE workspace_membership_id = $1",
            self.membership_id
        )
        .execute(&app_state.db_pool)
        .await?;

        // Delete membership
        sqlx::query!(
            "DELETE FROM workspace_memberships WHERE id = $1",
            self.membership_id
        )
        .execute(&app_state.db_pool)
        .await?;

        // Update workspace member count
        sqlx::query!(
            "UPDATE workspaces SET member_count = member_count - 1 WHERE id = $1",
            self.workspace_id
        )
        .execute(&app_state.db_pool)
        .await?;

        Ok(())
    }
}