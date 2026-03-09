use super::*;

pub struct ListOAuthAppsByDeploymentQuery {
    pub deployment_id: i64,
}

impl ListOAuthAppsByDeploymentQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<OAuthAppData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            r#"
            SELECT
                id,
                deployment_id,
                slug,
                name,
                description,
                logo_url,
                fqdn,
                supported_scopes as "supported_scopes: serde_json::Value",
                scope_definitions as "scope_definitions: serde_json::Value",
                allow_dynamic_client_registration,
                is_active,
                created_at,
                updated_at
            FROM oauth_apps
            WHERE deployment_id = $1
            ORDER BY created_at DESC
            "#,
            self.deployment_id
        )
        .fetch_all(executor)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| OAuthAppData {
                id: r.id,
                deployment_id: r.deployment_id,
                slug: r.slug,
                name: r.name,
                description: r.description,
                logo_url: r.logo_url,
                fqdn: r.fqdn,
                supported_scopes: r.supported_scopes,
                scope_definitions: r.scope_definitions,
                allow_dynamic_client_registration: r.allow_dynamic_client_registration,
                is_active: r.is_active,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }
}

pub struct GetOAuthAppBySlugQuery {
    pub deployment_id: i64,
    pub oauth_app_slug: String,
}

impl GetOAuthAppBySlugQuery {
    pub fn new(deployment_id: i64, oauth_app_slug: String) -> Self {
        Self {
            deployment_id,
            oauth_app_slug,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<OAuthAppData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT
                id,
                deployment_id,
                slug,
                name,
                description,
                logo_url,
                fqdn,
                supported_scopes as "supported_scopes: serde_json::Value",
                scope_definitions as "scope_definitions: serde_json::Value",
                allow_dynamic_client_registration,
                is_active,
                created_at,
                updated_at
            FROM oauth_apps
            WHERE deployment_id = $1
              AND slug = $2
            "#,
            self.deployment_id,
            self.oauth_app_slug
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| OAuthAppData {
            id: r.id,
            deployment_id: r.deployment_id,
            slug: r.slug,
            name: r.name,
            description: r.description,
            logo_url: r.logo_url,
            fqdn: r.fqdn,
            supported_scopes: r.supported_scopes,
            scope_definitions: r.scope_definitions,
            allow_dynamic_client_registration: r.allow_dynamic_client_registration,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }
}
