use super::*;

pub struct UpdateOAuthAppCommand {
    pub deployment_id: i64,
    pub oauth_app_slug: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub supported_scopes: Option<Vec<String>>,
    pub scope_definitions: Option<Vec<OAuthScopeDefinition>>,
    pub allow_dynamic_client_registration: Option<bool>,
    pub is_active: Option<bool>,
}

impl UpdateOAuthAppCommand {
    pub async fn execute_with_db<'a, Db>(self, db: Db) -> Result<OAuthAppData, AppError>
    where
        Db: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = db.begin().await?;
        let current = sqlx::query!(
            r#"
            SELECT
                supported_scopes as "supported_scopes: serde_json::Value",
                scope_definitions as "scope_definitions: serde_json::Value"
            FROM oauth_apps
            WHERE deployment_id = $1
              AND slug = $2
            "#,
            self.deployment_id,
            self.oauth_app_slug
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("OAuth app not found".to_string()))?;

        let current_supported_scopes: Vec<String> = json_default(current.supported_scopes);
        let supported_scopes = self.supported_scopes.unwrap_or(current_supported_scopes);
        let normalized_supported_scopes = normalize_supported_scopes(supported_scopes);
        let scope_definitions =
            normalize_scope_definitions(&normalized_supported_scopes, self.scope_definitions)?;

        let row = sqlx::query!(
            r#"
            UPDATE oauth_apps
            SET
                name = COALESCE($3, name),
                description = COALESCE($4, description),
                supported_scopes = COALESCE($5, supported_scopes),
                scope_definitions = COALESCE($6, scope_definitions),
                allow_dynamic_client_registration = COALESCE($7, allow_dynamic_client_registration),
                is_active = COALESCE($8, is_active),
                updated_at = NOW()
            WHERE deployment_id = $1
              AND slug = $2
            RETURNING
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
            "#,
            self.deployment_id,
            self.oauth_app_slug,
            self.name,
            self.description,
            serde_json::to_value(&normalized_supported_scopes)?,
            serde_json::to_value(&scope_definitions)?,
            self.allow_dynamic_client_registration,
            self.is_active
        )
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(OAuthAppData {
            id: row.id,
            deployment_id: row.deployment_id,
            slug: row.slug,
            name: row.name,
            description: row.description,
            logo_url: row.logo_url,
            fqdn: row.fqdn,
            supported_scopes: row.supported_scopes,
            scope_definitions: row.scope_definitions,
            allow_dynamic_client_registration: row.allow_dynamic_client_registration,
            is_active: row.is_active,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}
