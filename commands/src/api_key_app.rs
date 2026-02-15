use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::api_key::ApiAuthApp;

pub struct CreateApiAuthAppCommand {
    pub deployment_id: i64,
    pub app_slug: String,
    pub name: String,
    pub key_prefix: String,
    pub description: Option<String>,
    pub rate_limit_scheme_slug: Option<String>,
}

impl CreateApiAuthAppCommand {
    pub fn new(deployment_id: i64, app_slug: String, name: String, key_prefix: String) -> Self {
        Self {
            deployment_id,
            app_slug,
            name,
            key_prefix,
            description: None,
            rate_limit_scheme_slug: None,
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
}

impl Command for CreateApiAuthAppCommand {
    type Output = ApiAuthApp;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let app_id = app_state.sf.next_id()? as i64;

        let rec = sqlx::query!(
            r#"
            INSERT INTO api_auth_apps (id, deployment_id, app_slug, name, key_prefix, description, rate_limit_scheme_slug)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, deployment_id, app_slug, name, key_prefix, description, is_active,
                      rate_limit_scheme_slug,
                      created_at, updated_at, deleted_at
            "#,
            app_id,
            self.deployment_id,
            self.app_slug,
            self.name,
            self.key_prefix,
            self.description,
            self.rate_limit_scheme_slug
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(ApiAuthApp {
            id: rec.id,
            deployment_id: rec.deployment_id,
            app_slug: rec.app_slug,
            name: rec.name,
            description: rec.description,
            is_active: rec.is_active.unwrap_or(true),
            key_prefix: rec.key_prefix,
            rate_limits: vec![],
            rate_limit_scheme_slug: rec.rate_limit_scheme_slug,
            created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
            updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
            deleted_at: rec.deleted_at,
        })
    }
}

pub struct UpdateApiAuthAppCommand {
    pub app_id: i64,
    pub deployment_id: i64,
    pub name: Option<String>,
    pub key_prefix: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
    pub rate_limit_scheme_slug: Option<String>,
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
                updated_at = NOW()
            WHERE id = $1 AND deployment_id = $2
            RETURNING id, deployment_id, app_slug, name, key_prefix, description, is_active,
                      rate_limit_scheme_slug,
                      created_at, updated_at, deleted_at
            "#,
            self.app_id,
            self.deployment_id,
            self.name,
            self.key_prefix,
            self.description,
            self.is_active,
            self.rate_limit_scheme_slug
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(ApiAuthApp {
            id: rec.id,
            deployment_id: rec.deployment_id,
            app_slug: rec.app_slug,
            name: rec.name,
            description: rec.description,
            is_active: rec.is_active.unwrap_or(true),
            key_prefix: rec.key_prefix,
            rate_limits: vec![],
            rate_limit_scheme_slug: rec.rate_limit_scheme_slug,
            created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
            updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
            deleted_at: rec.deleted_at,
        })
    }
}

pub struct DeleteApiAuthAppCommand {
    pub app_id: i64,
    pub deployment_id: i64,
}

impl Command for DeleteApiAuthAppCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = sqlx::query!(
            r#"
            UPDATE api_auth_apps
            SET deleted_at = NOW()
            WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL
            "#,
            self.app_id,
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
