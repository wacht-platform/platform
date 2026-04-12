use common::error::AppError;
use models::api_key::ApiAuthApp;
use queries::api_key::GetApiAuthAppBySlugQuery;

use super::shared::{
    build_api_auth_app_model, ensure_user_in_organization, ensure_user_in_workspace,
    resolve_scope_organization,
};

pub struct UpdateApiAuthAppCommand {
    pub app_slug: String,
    pub deployment_id: i64,
    pub organization_id: Option<i64>,
    pub workspace_id: Option<i64>,
    pub name: Option<String>,
    pub key_prefix: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
    pub rate_limit_scheme_slug: Option<String>,
    pub permissions: Option<Vec<String>>,
    pub resources: Option<Vec<String>>,
}

impl UpdateApiAuthAppCommand {
    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<ApiAuthApp, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = acquirer.begin().await?;
        let current = GetApiAuthAppBySlugQuery::new(self.deployment_id, self.app_slug.clone())
            .execute_with_db(&mut *tx)
            .await?
            .ok_or_else(|| AppError::NotFound("API auth app not found".to_string()))?;

        let next_organization_id = self.organization_id.or(current.organization_id);
        let next_workspace_id = self.workspace_id.or(current.workspace_id);
        let next_organization_id = resolve_scope_organization(
            tx.as_mut(),
            self.deployment_id,
            next_organization_id,
            next_workspace_id,
        )
        .await?;

        if let Some(user_id) = current.user_id {
            if let Some(org_id) = next_organization_id {
                ensure_user_in_organization(tx.as_mut(), user_id, org_id).await?;
            }
            if let Some(workspace_id) = next_workspace_id {
                ensure_user_in_workspace(tx.as_mut(), user_id, workspace_id).await?;
            }
        } else if next_organization_id.is_some() || next_workspace_id.is_some() {
            return Err(AppError::Validation(
                "organization_id/workspace_id cannot be set when app has no user_id".to_string(),
            ));
        }

        let rec = sqlx::query!(
            r#"
            UPDATE api_auth_apps
            SET
                organization_id = $3,
                workspace_id = $4,
                name = COALESCE($5, name),
                key_prefix = COALESCE($6, key_prefix),
                description = COALESCE($7, description),
                is_active = COALESCE($8, is_active),
                rate_limit_scheme_slug = COALESCE($9, rate_limit_scheme_slug),
                permissions = COALESCE($10, permissions),
                resources = COALESCE($11, resources),
                updated_at = NOW()
            WHERE app_slug = $1 AND deployment_id = $2
            RETURNING deployment_id, user_id, organization_id, workspace_id, app_slug, name, key_prefix, description, is_active,
                      rate_limit_scheme_slug, permissions as "permissions: serde_json::Value", resources as "resources: serde_json::Value",
                      created_at, updated_at, deleted_at
            "#,
            self.app_slug,
            self.deployment_id,
            next_organization_id,
            next_workspace_id,
            self.name,
            self.key_prefix,
            self.description,
            self.is_active,
            self.rate_limit_scheme_slug,
            self.permissions.map(|v| serde_json::to_value(v)).transpose()?,
            self.resources.map(|v| serde_json::to_value(v)).transpose()?
        )
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            UPDATE api_keys
            SET rate_limit_scheme_slug = $1,
                updated_at = NOW()
            WHERE deployment_id = $2
              AND app_slug = $3
            "#,
            rec.rate_limit_scheme_slug,
            rec.deployment_id,
            rec.app_slug
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(build_api_auth_app_model(
            rec.deployment_id,
            rec.user_id,
            rec.organization_id,
            rec.workspace_id,
            rec.app_slug,
            rec.name,
            rec.description,
            rec.is_active,
            rec.key_prefix,
            rec.permissions,
            rec.resources,
            rec.rate_limit_scheme_slug,
            rec.created_at,
            rec.updated_at,
            rec.deleted_at,
        ))
    }
}
