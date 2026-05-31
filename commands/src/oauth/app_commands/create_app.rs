use super::*;

pub struct CreateOAuthAppCommand {
    pub oauth_app_id: Option<i64>,
    pub deployment_id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub logo_url: Option<String>,
    pub fqdn: Option<String>,
    pub supported_scopes: Vec<String>,
    pub scope_definitions: Option<Vec<OAuthScopeDefinition>>,
    pub allow_dynamic_client_registration: bool,
}

impl CreateOAuthAppCommand {
    pub fn with_oauth_app_id(mut self, oauth_app_id: i64) -> Self {
        self.oauth_app_id = Some(oauth_app_id);
        self
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<OAuthAppData, AppError>
    where
        D: HasDbRouter + HasCloudflareProvider,
    {
        let writer = deps.db_router().writer();
        let oauth_app_id = self
            .oauth_app_id
            .ok_or_else(|| AppError::Validation("oauth_app_id is required".to_string()))?;
        let cloudflare_service = deps.cloudflare_provider();
        let deployment = sqlx::query!(
            r#"
            SELECT mode
            FROM deployments
            WHERE id = $1
              AND deleted_at IS NULL
            "#,
            self.deployment_id
        )
        .fetch_optional(writer)
        .await?
        .ok_or_else(|| AppError::NotFound("Deployment not found".to_string()))?;

        let fqdn = build_oauth_fqdn(&deployment.mode, self.fqdn.as_deref())?;

        let cloudflare_custom_hostname_id: Option<String> =
            if deployment.mode.eq_ignore_ascii_case("production") {
                Some(
                    cloudflare_service
                        .create_custom_hostname(&fqdn, "oauth.wacht.services")
                        .await?
                        .id,
                )
            } else {
                None
            };

        let supported_scopes = normalize_supported_scopes(self.supported_scopes);
        let scope_definitions =
            normalize_scope_definitions(&supported_scopes, self.scope_definitions)?;
        let row_result = sqlx::query!(
            r#"
            INSERT INTO oauth_apps (
                id,
                deployment_id,
                slug,
                name,
                description,
                logo_url,
                fqdn,
                supported_scopes,
                scope_definitions,
                allow_dynamic_client_registration,
                is_active
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,true)
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
            oauth_app_id,
            self.deployment_id,
            self.slug,
            self.name,
            self.description,
            self.logo_url,
            fqdn,
            serde_json::to_value(&supported_scopes)?,
            serde_json::to_value(&scope_definitions)?,
            self.allow_dynamic_client_registration
        )
        .fetch_one(writer)
        .await;

        let row = match row_result {
            Ok(row) => row,
            Err(e) => {
                if let Some(custom_hostname_id) = cloudflare_custom_hostname_id {
                    let _ = cloudflare_service
                        .delete_custom_hostname(&custom_hostname_id)
                        .await;
                }
                return Err(e.into());
            }
        };

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
