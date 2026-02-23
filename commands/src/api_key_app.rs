use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::api_key::ApiAuthApp;
use sqlx::Row;

pub struct CreateApiAuthAppCommand {
    pub deployment_id: i64,
    pub user_id: Option<i64>,
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
        let rec = sqlx::query!(
            r#"
            INSERT INTO api_auth_apps (deployment_id, user_id, app_slug, name, key_prefix, description, rate_limit_scheme_slug, permissions, resources)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING deployment_id, user_id, app_slug, name, key_prefix, description, is_active,
                      rate_limit_scheme_slug, permissions as "permissions: serde_json::Value", resources as "resources: serde_json::Value",
                      created_at, updated_at, deleted_at
            "#,
            self.deployment_id,
            self.user_id,
            self.app_slug,
            self.name,
            self.key_prefix,
            self.description,
            self.rate_limit_scheme_slug,
            serde_json::to_value(&self.permissions)?,
            serde_json::to_value(&self.resources)?
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(ApiAuthApp {
            deployment_id: rec.deployment_id,
            user_id: rec.user_id,
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
        let rec = sqlx::query!(
            r#"
            UPDATE api_auth_apps
            SET
                name = COALESCE($3, name),
                key_prefix = COALESCE($4, key_prefix),
                description = COALESCE($5, description),
                is_active = COALESCE($6, is_active),
                rate_limit_scheme_slug = COALESCE($7, rate_limit_scheme_slug),
                permissions = COALESCE($8, permissions),
                resources = COALESCE($9, resources),
                updated_at = NOW()
            WHERE app_slug = $1 AND deployment_id = $2
            RETURNING deployment_id, user_id, app_slug, name, key_prefix, description, is_active,
                      rate_limit_scheme_slug, permissions as "permissions: serde_json::Value", resources as "resources: serde_json::Value",
                      created_at, updated_at, deleted_at
            "#,
            self.app_slug,
            self.deployment_id,
            self.name,
            self.key_prefix,
            self.description,
            self.is_active,
            self.rate_limit_scheme_slug,
            self.permissions.map(|v| serde_json::to_value(v)).transpose()?,
            self.resources.map(|v| serde_json::to_value(v)).transpose()?
        )
        .fetch_one(&app_state.db_pool)
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
        .execute(&app_state.db_pool)
        .await?;

        Ok(ApiAuthApp {
            deployment_id: rec.deployment_id,
            user_id: rec.user_id,
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
        let result = sqlx::query!(
            r#"
            UPDATE api_auth_apps
            SET deleted_at = NOW()
            WHERE app_slug = $1 AND deployment_id = $2 AND deleted_at IS NULL
            "#,
            self.app_slug,
            self.deployment_id
        )
        .execute(&app_state.db_pool)
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
        if self.user_id <= 0 {
            return Err(AppError::BadRequest(
                "user_id must be a positive integer".to_string(),
            ));
        }

        const MAX_CREATE_ATTEMPTS: usize = 5;
        for attempt in 0..MAX_CREATE_ATTEMPTS {
            let existing = sqlx::query(
                r#"
                SELECT app_slug
                FROM api_auth_apps
                WHERE deployment_id = $1
                  AND user_id = $2
                  AND deleted_at IS NULL
                ORDER BY created_at DESC
                LIMIT 1
                "#,
            )
            .bind(self.deployment_id)
            .bind(self.user_id)
            .fetch_optional(&app_state.db_pool)
            .await?;

            if let Some(row) = existing {
                let app_slug: String = row.get("app_slug");
                return Ok(app_slug);
            }

            let app_slug = if attempt == 0 {
                format!("oauth-user-{}", self.user_id)
            } else {
                format!("oauth-user-{}-{}", self.user_id, app_state.sf.next_id()?)
            };

            let create_result = CreateApiAuthAppCommand::new(
                self.deployment_id,
                Some(self.user_id),
                app_slug,
                format!("OAuth identity for user {}", self.user_id),
                "sk_live".to_string(),
            )
            .execute(app_state)
            .await;

            match create_result {
                Ok(created) => return Ok(created.app_slug),
                Err(AppError::Database(sqlx::Error::Database(db_err)))
                    if db_err.code().as_deref() == Some("23505") => {}
                Err(err) => return Err(err),
            }
        }

        Err(AppError::Internal(
            "failed to provision api auth app for consenting user".to_string(),
        ))
    }
}
