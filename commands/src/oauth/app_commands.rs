use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::api_key::OAuthScopeDefinition;
use queries::oauth::OAuthAppData;

pub struct CreateOAuthAppCommand {
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

impl Command for CreateOAuthAppCommand {
    type Output = OAuthAppData;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(
            &app_state.db_pool,
            &app_state.cloudflare_service,
            app_state.sf.next_id()? as i64,
        )
        .await
    }
}

impl CreateOAuthAppCommand {
    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        cloudflare_service: &common::CloudflareService,
        oauth_app_id: i64,
    ) -> Result<OAuthAppData, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_connection(conn, cloudflare_service, oauth_app_id)
            .await
    }

    async fn execute_with_connection<C>(
        self,
        mut conn: C,
        cloudflare_service: &common::CloudflareService,
        oauth_app_id: i64,
    ) -> Result<OAuthAppData, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
        let deployment = sqlx::query!(
            r#"
            SELECT mode
            FROM deployments
            WHERE id = $1
              AND deleted_at IS NULL
            "#,
            self.deployment_id
        )
        .fetch_optional(&mut *conn)
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
        .fetch_one(&mut *conn)
        .await;

        let row = match row_result {
            Ok(row) => row,
            Err(e) => {
                if let Some(custom_hostname_id) = cloudflare_custom_hostname_id {
                    let _ = cloudflare_service.delete_custom_hostname(&custom_hostname_id).await;
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

pub struct VerifyOAuthAppDomainResult {
    pub domain: String,
    pub cname_target: String,
    pub verified: bool,
}

pub struct VerifyOAuthAppDomainCommand {
    pub deployment_id: i64,
    pub oauth_app_slug: String,
}

impl Command for VerifyOAuthAppDomainCommand {
    type Output = VerifyOAuthAppDomainResult;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool, &app_state.cloudflare_service)
            .await
    }
}

impl VerifyOAuthAppDomainCommand {
    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        cloudflare_service: &common::CloudflareService,
    ) -> Result<VerifyOAuthAppDomainResult, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_connection(conn, cloudflare_service).await
    }

    async fn execute_with_connection<C>(
        self,
        mut conn: C,
        cloudflare_service: &common::CloudflareService,
    ) -> Result<VerifyOAuthAppDomainResult, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
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
        .fetch_optional(&mut *conn)
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

impl Command for UpdateOAuthAppCommand {
    type Output = OAuthAppData;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}

impl UpdateOAuthAppCommand {
    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
    ) -> Result<OAuthAppData, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_connection(conn).await
    }

    async fn execute_with_connection<C>(self, mut conn: C) -> Result<OAuthAppData, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
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
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound("OAuth app not found".to_string()))?;

        let current_supported_scopes: Vec<String> =
            serde_json::from_value(current.supported_scopes).unwrap_or_default();
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
        .fetch_one(&mut *conn)
        .await?;

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

fn normalize_supported_scopes(scopes: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = scopes
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    out.sort();
    out.dedup();
    out
}

fn normalize_scope_definitions(
    supported_scopes: &[String],
    input: Option<Vec<OAuthScopeDefinition>>,
) -> Result<Vec<OAuthScopeDefinition>, AppError> {
    let mut by_scope = std::collections::BTreeMap::<String, OAuthScopeDefinition>::new();

    if let Some(defs) = input {
        for mut def in defs {
            let scope = def.scope.trim().to_string();
            if scope.is_empty() {
                continue;
            }
            def.scope = scope.clone();
            by_scope.insert(scope, def);
        }
    }

    supported_scopes
        .iter()
        .map(|scope| {
            if let Some(mut def) = by_scope.remove(scope) {
                if def.display_name.trim().is_empty() {
                    def.display_name = scope.to_string();
                }
                if def.description.trim().is_empty() {
                    def.description = format!("Allows {} access", scope);
                }
                def.category = parse_scope_category(def.category)?;
                def.organization_permission = def
                    .organization_permission
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(ToOwned::to_owned);
                def.workspace_permission = def
                    .workspace_permission
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(ToOwned::to_owned);
                validate_scope_mapping(scope, &def)?;
                return Ok(def);
            }

            let def = OAuthScopeDefinition {
                scope: scope.to_string(),
                display_name: scope.to_string(),
                description: format!("Allows {} access", scope),
                archived: false,
                category: String::new(),
                organization_permission: None,
                workspace_permission: None,
            };
            validate_scope_mapping(scope, &def)?;
            Ok(def)
        })
        .collect::<Result<Vec<_>, AppError>>()
}

fn parse_scope_category(raw: String) -> Result<String, AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" => Ok(String::new()),
        "personal" => Ok("personal".to_string()),
        "organization" => Ok("organization".to_string()),
        "workspace" => Ok("workspace".to_string()),
        _ => Err(AppError::Validation(
            "scope category must be one of: personal, organization, workspace".to_string(),
        )),
    }
}

fn validate_scope_mapping(scope: &str, def: &OAuthScopeDefinition) -> Result<(), AppError> {
    match def.category.as_str() {
        "" => {
            if def.organization_permission.is_some() || def.workspace_permission.is_some() {
                return Err(AppError::Validation(format!(
                    "scope '{}' without category cannot define permission mappings",
                    scope
                )));
            }
        }
        "personal" => {
            if def.organization_permission.is_some() || def.workspace_permission.is_some() {
                return Err(AppError::Validation(format!(
                    "scope '{}' with category 'personal' cannot define organization/workspace permissions",
                    scope
                )));
            }
        }
        "organization" => {
            if def.workspace_permission.is_some() {
                return Err(AppError::Validation(format!(
                    "scope '{}' with category 'organization' cannot define workspace permission",
                    scope
                )));
            }
        }
        "workspace" => {
            if def.organization_permission.is_some() {
                return Err(AppError::Validation(format!(
                    "scope '{}' with category 'workspace' cannot define organization permission",
                    scope
                )));
            }
        }
        _ => {
            return Err(AppError::Validation(format!(
                "scope '{}' has invalid category '{}'",
                scope, def.category
            )));
        }
    }
    Ok(())
}

fn build_oauth_fqdn(mode: &str, fqdn: Option<&str>) -> Result<String, AppError> {
    if mode.eq_ignore_ascii_case("production") {
        let value = fqdn
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| AppError::Validation("fqdn is required for production".to_string()))?;
        validate_fqdn(value)?;
        return Ok(value.to_ascii_lowercase());
    }

    let label = generate_oauth_domain_label();
    validate_dns_label(&label, "fqdn")?;
    Ok(format!("{}.oapi.trywacht.xyz", label))
}

fn validate_dns_label(value: &str, field_name: &str) -> Result<(), AppError> {
    if value.is_empty() || value.len() > 63 {
        return Err(AppError::Validation(format!(
            "{} must be between 1 and 63 characters",
            field_name
        )));
    }

    if value.starts_with('-') || value.ends_with('-') {
        return Err(AppError::Validation(format!(
            "{} cannot start or end with '-'",
            field_name
        )));
    }

    if !value.bytes().all(|b| {
        b.is_ascii_lowercase() || b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-'
    }) {
        return Err(AppError::Validation(format!(
            "{} must contain only letters, numbers, or '-'",
            field_name
        )));
    }

    Ok(())
}

fn generate_oauth_domain_label() -> String {
    const EDGE_ALPHABET: [char; 36] = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
        's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
    ];
    const MIDDLE_ALPHABET: [char; 37] = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
        's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
        '-',
    ];
    const LABEL_LEN: usize = 16;
    const MIDDLE_LEN: usize = LABEL_LEN - 2;
    let first = nanoid::nanoid!(1, &EDGE_ALPHABET);
    let middle = nanoid::nanoid!(MIDDLE_LEN, &MIDDLE_ALPHABET);
    let last = nanoid::nanoid!(1, &EDGE_ALPHABET);
    format!("{}{}{}", first, middle, last)
}

fn validate_fqdn(value: &str) -> Result<(), AppError> {
    if value.is_empty() || value.len() > 253 {
        return Err(AppError::Validation(
            "fqdn must be between 1 and 253 characters".to_string(),
        ));
    }
    if value.starts_with('.') || value.ends_with('.') {
        return Err(AppError::Validation(
            "fqdn cannot start or end with '.'".to_string(),
        ));
    }
    if !value.contains('.') {
        return Err(AppError::Validation(
            "fqdn must be a full domain name".to_string(),
        ));
    }

    for label in value.split('.') {
        validate_dns_label(label, "fqdn")?;
    }

    Ok(())
}
