use common::error::AppError;
use models::api_key::ApiAuthApp;

use super::shared::{
    build_api_auth_app_model, ensure_user_exists, ensure_user_in_organization,
    ensure_user_in_workspace, resolve_scope_organization,
};

pub struct CreateApiAuthAppCommand {
    pub deployment_id: i64,
    pub user_id: Option<i64>,
    pub organization_id: Option<i64>,
    pub workspace_id: Option<i64>,
    pub app_slug: String,
    pub name: String,
    pub key_prefix: String,
    pub description: Option<String>,
    pub rate_limit_scheme_slug: Option<String>,
    pub permissions: Vec<String>,
    pub resources: Vec<String>,
}

impl CreateApiAuthAppCommand {
    pub fn new(
        deployment_id: i64,
        user_id: Option<i64>,
        app_slug: String,
        name: String,
        key_prefix: String,
    ) -> Self {
        Self {
            deployment_id,
            user_id,
            organization_id: None,
            workspace_id: None,
            app_slug,
            name,
            key_prefix,
            description: None,
            rate_limit_scheme_slug: None,
            permissions: vec![],
            resources: vec![],
        }
    }

    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    pub fn with_rate_limit_scheme_slug(mut self, slug: Option<String>) -> Self {
        self.rate_limit_scheme_slug = slug;
        self
    }

    pub fn with_scope(mut self, organization_id: Option<i64>, workspace_id: Option<i64>) -> Self {
        self.organization_id = organization_id;
        self.workspace_id = workspace_id;
        self
    }

    pub fn with_permissions(mut self, permissions: Vec<String>) -> Self {
        self.permissions = permissions;
        self
    }

    pub fn with_resources(mut self, resources: Vec<String>) -> Self {
        self.resources = resources;
        self
    }
}

impl CreateApiAuthAppCommand {
    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<ApiAuthApp, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = acquirer.begin().await?;
        let organization_id = resolve_scope_organization(
            tx.as_mut(),
            self.deployment_id,
            self.organization_id,
            self.workspace_id,
        )
        .await?;

        if let Some(user_id) = self.user_id {
            ensure_user_exists(tx.as_mut(), self.deployment_id, user_id).await?;
            if let Some(org_id) = organization_id {
                ensure_user_in_organization(tx.as_mut(), user_id, org_id).await?;
            }
            if let Some(workspace_id) = self.workspace_id {
                ensure_user_in_workspace(tx.as_mut(), user_id, workspace_id).await?;
            }
        } else if organization_id.is_some() || self.workspace_id.is_some() {
            return Err(AppError::Validation(
                "user_id is required when organization_id/workspace_id is provided".to_string(),
            ));
        }

        let rec = sqlx::query!(
            r#"
            INSERT INTO api_auth_apps (deployment_id, user_id, organization_id, workspace_id, app_slug, name, key_prefix, description, rate_limit_scheme_slug, permissions, resources)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING deployment_id, user_id, organization_id, workspace_id, app_slug, name, key_prefix, description, is_active,
                      rate_limit_scheme_slug, permissions as "permissions: serde_json::Value", resources as "resources: serde_json::Value",
                      created_at, updated_at, deleted_at
            "#,
            self.deployment_id,
            self.user_id,
            organization_id,
            self.workspace_id,
            self.app_slug,
            self.name,
            self.key_prefix,
            self.description,
            self.rate_limit_scheme_slug,
            serde_json::to_value(&self.permissions)?,
            serde_json::to_value(&self.resources)?
        )
        .fetch_one(&mut *tx)
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
