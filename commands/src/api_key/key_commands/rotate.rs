use common::error::AppError;
use models::api_key::ApiKeyWithSecret;
use queries::api_key::{
    GetApiAuthAppBySlugQuery, GetOrganizationMembershipPermissionsQuery,
    GetWorkspaceMembershipPermissionsQuery,
};

use super::create::CreateApiKeyCommand;
use super::shared::{
    build_api_key_model, resolve_org_membership_id, resolve_workspace_membership_id,
    user_not_member_error,
};

pub struct RotateApiKeyCommand {
    pub key_id: i64,
    pub deployment_id: i64,
    pub new_key_id: Option<i64>,
}

impl RotateApiKeyCommand {
    pub fn with_new_key_id(mut self, new_key_id: i64) -> Self {
        self.new_key_id = Some(new_key_id);
        self
    }

    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<ApiKeyWithSecret, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = acquirer.begin().await?;
        let new_key_id = self
            .new_key_id
            .ok_or_else(|| AppError::Validation("new_key_id is required".to_string()))?;
        // Get the existing key
        let rec = sqlx::query!(
            r#"
            SELECT id, deployment_id, app_slug, name, key_prefix, key_suffix,
                   permissions as "permissions: serde_json::Value",
                   metadata as "metadata: serde_json::Value",
                   rate_limit_scheme_slug,
                   owner_user_id,
                   organization_id, workspace_id, organization_membership_id, workspace_membership_id,
                   org_role_permissions as "org_role_permissions: serde_json::Value",
                   workspace_role_permissions as "workspace_role_permissions: serde_json::Value",
                   expires_at
            FROM api_keys
            WHERE id = $1 AND deployment_id = $2 AND is_active = true
            "#,
            self.key_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("API key not found or inactive".to_string()))?;

        let existing_key = build_api_key_model(
            rec.id,
            rec.deployment_id,
            rec.app_slug,
            rec.name,
            rec.key_prefix,
            rec.key_suffix,
            String::new(), // Not needed for rotation
            rec.permissions.clone(),
            rec.metadata.clone(),
            rec.rate_limit_scheme_slug,
            rec.owner_user_id,
            rec.organization_id,
            rec.workspace_id,
            rec.organization_membership_id,
            rec.workspace_membership_id,
            rec.org_role_permissions.clone(),
            rec.workspace_role_permissions.clone(),
            rec.expires_at,
            None,
            Some(true),
            None,
            None,
            None,
            None,
        );

        let app_context =
            GetApiAuthAppBySlugQuery::new(self.deployment_id, existing_key.app_slug.clone())
                .execute_with_db(&mut *tx)
                .await?
                .filter(|app| app.is_active)
                .ok_or_else(|| {
                    AppError::NotFound("API key app not found or inactive".to_string())
                })?;

        if app_context.user_id.is_none()
            && (app_context.organization_id.is_some() || app_context.workspace_id.is_some())
        {
            return Err(user_not_member_error());
        }

        let org_membership_id =
            resolve_org_membership_id(&mut tx, app_context.user_id, app_context.organization_id)
                .await?;
        let workspace_membership_id =
            resolve_workspace_membership_id(&mut tx, app_context.user_id, app_context.workspace_id)
                .await?;

        let mut organization_id: Option<i64> = None;
        let mut workspace_id: Option<i64> = None;
        let mut org_role_permissions: Vec<String> = vec![];
        let mut workspace_role_permissions: Vec<String> = vec![];

        if let Some(org_membership_id) = org_membership_id {
            let org_perm = GetOrganizationMembershipPermissionsQuery::new(org_membership_id)
                .execute_with_db(&mut *tx)
                .await?
                .ok_or_else(|| {
                    AppError::NotFound("Organization membership not found".to_string())
                })?;

            organization_id = Some(org_perm.organization_id);
            org_role_permissions = org_perm.permissions;
        }

        if let Some(workspace_membership_id) = workspace_membership_id {
            let workspace_perm =
                GetWorkspaceMembershipPermissionsQuery::new(workspace_membership_id)
                    .execute_with_db(&mut *tx)
                    .await?
                    .ok_or_else(|| {
                        AppError::NotFound("Workspace membership not found".to_string())
                    })?;

            if let Some(existing_org_id) = organization_id {
                if existing_org_id != workspace_perm.organization_id {
                    return Err(AppError::BadRequest(
                        "organization_membership_id and workspace_membership_id belong to different organizations"
                            .to_string(),
                    ));
                }
            }

            organization_id = Some(workspace_perm.organization_id);
            workspace_id = Some(workspace_perm.workspace_id);
            workspace_role_permissions = workspace_perm.permissions;
        }

        // Revoke the old key
        sqlx::query!(
            r#"
            UPDATE api_keys
            SET
                is_active = false,
                revoked_at = NOW(),
                revoked_reason = 'Rotated',
                updated_at = NOW()
            WHERE id = $1
            "#,
            self.key_id
        )
        .execute(&mut *tx)
        .await?;

        // Create a new key with the same settings
        let create_command = CreateApiKeyCommand {
            key_id: Some(new_key_id),
            app_slug: existing_key.app_slug,
            deployment_id: existing_key.deployment_id,
            name: existing_key.name,
            key_prefix: existing_key.key_prefix,
            permissions: existing_key.permissions,
            metadata: Some(existing_key.metadata),
            expires_at: existing_key.expires_at,
            rate_limit_scheme_slug: existing_key.rate_limit_scheme_slug,
            owner_user_id: app_context.user_id,
            organization_id,
            workspace_id,
            organization_membership_id: org_membership_id,
            workspace_membership_id,
            org_role_permissions,
            workspace_role_permissions,
        };

        let result = create_command.execute_with_db(tx.as_mut()).await?;
        tx.commit().await?;
        Ok(result)
    }
}
