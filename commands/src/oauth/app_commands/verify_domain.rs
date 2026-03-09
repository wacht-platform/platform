use super::*;

pub struct VerifyOAuthAppDomainResult {
    pub domain: String,
    pub cname_target: String,
    pub verified: bool,
}

pub struct VerifyOAuthAppDomainCommand {
    pub deployment_id: i64,
    pub oauth_app_slug: String,
}

impl VerifyOAuthAppDomainCommand {
    pub async fn execute_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<VerifyOAuthAppDomainResult, AppError>
    where
        D: HasDbRouter + HasCloudflareProvider,
    {
        let writer = deps.db_router().writer();
        let cloudflare_service = deps.cloudflare_provider();
        let oauth_app = sqlx::query!(
            r#"
            SELECT fqdn
            FROM oauth_apps
            WHERE deployment_id = $1
              AND slug = $2
            "#,
            self.deployment_id,
            self.oauth_app_slug
        )
        .fetch_optional(writer)
        .await?
        .ok_or_else(|| AppError::NotFound("OAuth app not found".to_string()))?;

        let verified = cloudflare_service
            .check_custom_hostname_status(&oauth_app.fqdn)
            .await?;

        Ok(VerifyOAuthAppDomainResult {
            domain: oauth_app.fqdn,
            cname_target: "oauth.wacht.services".to_string(),
            verified,
        })
    }
}
