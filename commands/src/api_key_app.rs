use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::api_key::ApiAuthApp;

async fn ensure_user_exists(
    conn: &mut sqlx::PgConnection,
    deployment_id: i64,
    user_id: i64,
) -> Result<(), AppError> {
    let user = sqlx::query!(
        r#"
        SELECT id
        FROM users
        WHERE id = $1
          AND deployment_id = $2
          AND deleted_at IS NULL
        LIMIT 1
        "#,
        user_id,
        deployment_id
    )
    .fetch_optional(&mut *conn)
    .await?;

    if user.is_none() {
        return Err(AppError::Validation(
            "user_id does not exist for this deployment".to_string(),
        ));
    }

    Ok(())
}

async fn ensure_organization_exists(
    conn: &mut sqlx::PgConnection,
    deployment_id: i64,
    organization_id: i64,
) -> Result<(), AppError> {
    let organization = sqlx::query!(
        r#"
        SELECT id
        FROM organizations
        WHERE id = $1
          AND deployment_id = $2
          AND deleted_at IS NULL
        LIMIT 1
        "#,
        organization_id,
        deployment_id
    )
    .fetch_optional(&mut *conn)
    .await?;

    if organization.is_none() {
        return Err(AppError::Validation(
            "organization_id does not exist for this deployment".to_string(),
        ));
    }

    Ok(())
}

async fn resolve_workspace_organization(
    conn: &mut sqlx::PgConnection,
    deployment_id: i64,
    workspace_id: i64,
) -> Result<i64, AppError> {
    let workspace = sqlx::query!(
        r#"
        SELECT organization_id
        FROM workspaces
        WHERE id = $1
          AND deployment_id = $2
          AND deleted_at IS NULL
        LIMIT 1
        "#,
        workspace_id,
        deployment_id
    )
    .fetch_optional(&mut *conn)
    .await?;

    workspace.map(|w| w.organization_id).ok_or_else(|| {
        AppError::Validation("workspace_id does not exist for this deployment".to_string())
    })
}

async fn ensure_user_in_organization(
    conn: &mut sqlx::PgConnection,
    user_id: i64,
    organization_id: i64,
) -> Result<(), AppError> {
    let membership = sqlx::query!(
        r#"
        SELECT id
        FROM organization_memberships
        WHERE user_id = $1
          AND organization_id = $2
          AND deleted_at IS NULL
        LIMIT 1
        "#,
        user_id,
        organization_id
    )
    .fetch_optional(&mut *conn)
    .await?;

    if membership.is_none() {
        return Err(AppError::Validation(
            "user_id is not a member of organization_id".to_string(),
        ));
    }

    Ok(())
}

async fn ensure_user_in_workspace(
    conn: &mut sqlx::PgConnection,
    user_id: i64,
    workspace_id: i64,
) -> Result<(), AppError> {
    let membership = sqlx::query!(
        r#"
        SELECT id
        FROM workspace_memberships
        WHERE user_id = $1
          AND workspace_id = $2
          AND deleted_at IS NULL
        LIMIT 1
        "#,
        user_id,
        workspace_id
    )
    .fetch_optional(&mut *conn)
    .await?;

    if membership.is_none() {
        return Err(AppError::Validation(
            "user_id is not a member of workspace_id".to_string(),
        ));
    }

    Ok(())
}

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

impl Command for CreateApiAuthAppCommand {
    type Output = ApiAuthApp;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}

impl CreateApiAuthAppCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<ApiAuthApp, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_connection(conn).await
    }

    async fn execute_with_connection<C>(self, mut conn: C) -> Result<ApiAuthApp, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
        let mut organization_id = self.organization_id;

        if let Some(workspace_id) = self.workspace_id {
            let workspace_org_id =
                resolve_workspace_organization(&mut *conn, self.deployment_id, workspace_id).await?;
            if let Some(explicit_org_id) = organization_id {
                if explicit_org_id != workspace_org_id {
                    return Err(AppError::Validation(
                        "workspace_id does not belong to organization_id".to_string(),
                    ));
                }
            }
            organization_id = Some(workspace_org_id);
        }

        if let Some(org_id) = organization_id {
            ensure_organization_exists(&mut *conn, self.deployment_id, org_id).await?;
        }

        if let Some(user_id) = self.user_id {
            ensure_user_exists(&mut *conn, self.deployment_id, user_id).await?;
            if let Some(org_id) = organization_id {
                ensure_user_in_organization(&mut *conn, user_id, org_id).await?;
            }
            if let Some(workspace_id) = self.workspace_id {
                ensure_user_in_workspace(&mut *conn, user_id, workspace_id).await?;
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
        .fetch_one(&mut *conn)
        .await?;

        Ok(ApiAuthApp {
            deployment_id: rec.deployment_id,
            user_id: rec.user_id,
            organization_id: rec.organization_id,
            workspace_id: rec.workspace_id,
            app_slug: rec.app_slug,
            name: rec.name,
            description: rec.description,
            is_active: rec.is_active.unwrap_or(true),
            key_prefix: rec.key_prefix,
            permissions: serde_json::from_value(rec.permissions.clone()).unwrap_or_default(),
            resources: serde_json::from_value(rec.resources.clone()).unwrap_or_default(),
            rate_limits: vec![],
            rate_limit_scheme_slug: rec.rate_limit_scheme_slug,
            created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
            updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
            deleted_at: rec.deleted_at,
        })
    }
}

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

impl Command for UpdateApiAuthAppCommand {
    type Output = ApiAuthApp;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}

impl UpdateApiAuthAppCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<ApiAuthApp, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_connection(conn).await
    }

    async fn execute_with_connection<C>(self, mut conn: C) -> Result<ApiAuthApp, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
        let current = sqlx::query!(
            r#"
            SELECT user_id, organization_id, workspace_id
            FROM api_auth_apps
            WHERE app_slug = $1 AND deployment_id = $2
            "#,
            self.app_slug,
            self.deployment_id
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound("API auth app not found".to_string()))?;

        let mut next_organization_id = self.organization_id.or(current.organization_id);
        let next_workspace_id = self.workspace_id.or(current.workspace_id);

        if let Some(workspace_id) = next_workspace_id {
            let workspace_org_id =
                resolve_workspace_organization(&mut *conn, self.deployment_id, workspace_id).await?;
            if let Some(explicit_org_id) = next_organization_id {
                if explicit_org_id != workspace_org_id {
                    return Err(AppError::Validation(
                        "workspace_id does not belong to organization_id".to_string(),
                    ));
                }
            }
            next_organization_id = Some(workspace_org_id);
        }

        if let Some(org_id) = next_organization_id {
            ensure_organization_exists(&mut *conn, self.deployment_id, org_id).await?;
        }

        if let Some(user_id) = current.user_id {
            if let Some(org_id) = next_organization_id {
                ensure_user_in_organization(&mut *conn, user_id, org_id).await?;
            }
            if let Some(workspace_id) = next_workspace_id {
                ensure_user_in_workspace(&mut *conn, user_id, workspace_id).await?;
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
        .fetch_one(&mut *conn)
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
        .execute(&mut *conn)
        .await?;

        Ok(ApiAuthApp {
            deployment_id: rec.deployment_id,
            user_id: rec.user_id,
            organization_id: rec.organization_id,
            workspace_id: rec.workspace_id,
            app_slug: rec.app_slug,
            name: rec.name,
            description: rec.description,
            is_active: rec.is_active.unwrap_or(true),
            key_prefix: rec.key_prefix,
            permissions: serde_json::from_value(rec.permissions.clone()).unwrap_or_default(),
            resources: serde_json::from_value(rec.resources.clone()).unwrap_or_default(),
            rate_limits: vec![],
            rate_limit_scheme_slug: rec.rate_limit_scheme_slug,
            created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
            updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
            deleted_at: rec.deleted_at,
        })
    }
}

pub struct DeleteApiAuthAppCommand {
    pub app_slug: String,
    pub deployment_id: i64,
}

impl Command for DeleteApiAuthAppCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}

impl DeleteApiAuthAppCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_connection(conn).await
    }

    async fn execute_with_connection<C>(self, mut conn: C) -> Result<(), AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
        let result = sqlx::query!(
            r#"
            UPDATE api_auth_apps
            SET deleted_at = NOW()
            WHERE app_slug = $1 AND deployment_id = $2 AND deleted_at IS NULL
            "#,
            self.app_slug,
            self.deployment_id
        )
        .execute(&mut *conn)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("API auth app not found".to_string()));
        }

        Ok(())
    }
}

pub struct EnsureUserApiAuthAppCommand {
    pub deployment_id: i64,
    pub user_id: i64,
}

impl EnsureUserApiAuthAppCommand {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }
}

impl Command for EnsureUserApiAuthAppCommand {
    type Output = String;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}

impl EnsureUserApiAuthAppCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<String, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_connection(conn).await
    }

    async fn execute_with_connection<C>(self, mut conn: C) -> Result<String, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
        if self.user_id <= 0 {
            return Err(AppError::BadRequest(
                "user_id must be a positive integer".to_string(),
            ));
        }

        let expected_slug = format!("oauth_{}", self.user_id);

        let existing = sqlx::query!(
            r#"
            SELECT app_slug as "app_slug!"
            FROM api_auth_apps
            WHERE deployment_id = $1
              AND app_slug = $2
              AND deleted_at IS NULL
            LIMIT 1
            "#,
            self.deployment_id,
            expected_slug
        )
        .fetch_optional(&mut *conn)
        .await?;

        if let Some(row) = existing {
            return Ok(row.app_slug);
        }

        let create_result = CreateApiAuthAppCommand::new(
            self.deployment_id,
            Some(self.user_id),
            expected_slug.clone(),
            format!("OAuth identity for user {}", self.user_id),
            "sk_live".to_string(),
        )
        .execute_with_connection(&mut *conn)
        .await;

        match create_result {
            Ok(created) => Ok(created.app_slug),
            Err(AppError::Database(sqlx::Error::Database(db_err)))
                if db_err.code().as_deref() == Some("23505") =>
            {
                Ok(expected_slug)
            }
            Err(err) => Err(err),
        }
    }
}
