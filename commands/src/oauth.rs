use crate::Command;
use chrono::{DateTime, Utc};
use common::error::AppError;
use common::state::AppState;
use models::api_key::{JwksDocument, OAuthScopeDefinition};
use queries::oauth::{OAuthAppData, OAuthClientData};
use sha2::{Digest, Sha256};

const REQUIRED_OAUTH_SCOPES: [&str; 2] = ["read", "write"];

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
        let deployment = sqlx::query!(
            r#"
            SELECT mode
            FROM deployments
            WHERE id = $1
              AND deleted_at IS NULL
            "#,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Deployment not found".to_string()))?;

        let fqdn = build_oauth_fqdn(&deployment.mode, self.fqdn.as_deref())?;

        let cloudflare_custom_hostname_id: Option<String> =
            if deployment.mode.eq_ignore_ascii_case("production") {
                Some(
                    app_state
                        .cloudflare_service
                        .create_custom_hostname(&fqdn, "oauth.wacht.services")
                        .await?
                        .id,
                )
            } else {
                None
            };

        let id = app_state.sf.next_id()? as i64;
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
            id,
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
        .fetch_one(&app_state.db_pool)
        .await;

        let row = match row_result {
            Ok(row) => row,
            Err(e) => {
                if let Some(custom_hostname_id) = cloudflare_custom_hostname_id {
                    let _ = app_state
                        .cloudflare_service
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

impl Command for UpdateOAuthAppCommand {
    type Output = OAuthAppData;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .fetch_optional(&app_state.db_pool)
        .await?
        .ok_or_else(|| AppError::NotFound("OAuth app not found".to_string()))?;

        let current_supported_scopes: Vec<String> =
            serde_json::from_value(current.supported_scopes).unwrap_or_default();
        let supported_scopes = self.supported_scopes.unwrap_or(current_supported_scopes);
        let normalized_supported_scopes = normalize_supported_scopes(supported_scopes);
        let scope_definitions = normalize_scope_definitions(
            &normalized_supported_scopes,
            self.scope_definitions,
        )?;

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
        .fetch_one(&app_state.db_pool)
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

    for required in REQUIRED_OAUTH_SCOPES {
        if !out.iter().any(|s| s == required) {
            out.push(required.to_string());
        }
    }

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
    Ok(format!("{}.o.feapis.xyz", label))
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

pub struct CreateOAuthClientCommand {
    pub deployment_id: i64,
    pub oauth_app_id: i64,
    pub client_auth_method: String,
    pub grant_types: Vec<String>,
    pub redirect_uris: Vec<String>,
    pub token_endpoint_auth_signing_alg: Option<String>,
    pub jwks_uri: Option<String>,
    pub jwks: Option<JwksDocument>,
    pub public_key_pem: Option<String>,
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub logo_uri: Option<String>,
    pub tos_uri: Option<String>,
    pub policy_uri: Option<String>,
    pub contacts: Option<Vec<String>>,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
}

pub struct OAuthClientWithSecret {
    pub client: OAuthClientData,
    pub client_secret: Option<String>,
}

impl CreateOAuthClientCommand {
    fn generate_client_id() -> String {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        use rand::RngCore;

        let mut random_bytes = [0u8; 24];
        rand::rng().fill_bytes(&mut random_bytes);
        format!("oc_{}", URL_SAFE_NO_PAD.encode(random_bytes))
    }

    fn validate_jwks_document(jwks: &JwksDocument) -> Result<(), AppError> {
        if jwks.keys.is_empty() {
            return Err(AppError::Validation(
                "jwks.keys must include at least one key".to_string(),
            ));
        }

        for key in &jwks.keys {
            if key.k.is_some() {
                return Err(AppError::Validation(
                    "jwks for private_key_jwt must contain public keys, not symmetric secrets"
                        .to_string(),
                ));
            }
        }

        Ok(())
    }

    fn validate(&self) -> Result<(), AppError> {
        if self.grant_types.is_empty() {
            return Err(AppError::Validation(
                "grant_types must include at least one grant".to_string(),
            ));
        }
        if self.grant_types.iter().any(|g| g == "client_credentials") {
            return Err(AppError::Validation(
                "client_credentials is disabled for now".to_string(),
            ));
        }
        let method = self.client_auth_method.as_str();
        let allowed = [
            "client_secret_basic",
            "client_secret_post",
            "client_secret_jwt",
            "none",
            "private_key_jwt",
        ];
        if !allowed.contains(&method) {
            return Err(AppError::Validation(format!(
                "unsupported client_auth_method: {}",
                self.client_auth_method
            )));
        }

        let has_jwks_uri = self
            .jwks_uri
            .as_deref()
            .is_some_and(|v| !v.trim().is_empty());
        let has_jwks = self.jwks.is_some();
        let has_public_key_pem = self
            .public_key_pem
            .as_deref()
            .is_some_and(|v| !v.trim().is_empty());

        if method == "private_key_jwt" {
            let key_material_count = has_jwks_uri as u8 + has_jwks as u8 + has_public_key_pem as u8;
            if key_material_count != 1 {
                return Err(AppError::Validation(
                    "private_key_jwt requires exactly one of jwks_uri, jwks, or public_key_pem"
                        .to_string(),
                ));
            }

            if let Some(jwks_uri) = &self.jwks_uri {
                let parsed = url::Url::parse(jwks_uri.trim()).map_err(|_| {
                    AppError::Validation("jwks_uri must be a valid URL".to_string())
                })?;
                if parsed.scheme() != "https" {
                    return Err(AppError::Validation("jwks_uri must use https".to_string()));
                }
            }

            if let Some(jwks) = &self.jwks {
                Self::validate_jwks_document(jwks)?;
            }
        } else if has_jwks_uri || has_jwks || has_public_key_pem {
            return Err(AppError::Validation(
                "jwks_uri/jwks/public_key_pem are only allowed when client_auth_method is private_key_jwt".to_string(),
            ));
        }

        validate_optional_url("client_uri", self.client_uri.as_deref())?;
        validate_optional_url("logo_uri", self.logo_uri.as_deref())?;
        validate_optional_url("tos_uri", self.tos_uri.as_deref())?;
        validate_optional_url("policy_uri", self.policy_uri.as_deref())?;
        validate_redirect_uris(&self.redirect_uris, &self.grant_types)?;

        Ok(())
    }

    fn generate_client_secret_hash_and_encrypted(
        &self,
        app_state: &AppState,
    ) -> Result<(Option<String>, Option<String>, Option<String>), AppError> {
        if self.client_auth_method == "none" || self.client_auth_method == "private_key_jwt" {
            return Ok((None, None, None));
        }

        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        use rand::RngCore;
        let mut random_bytes = vec![0u8; 32];
        rand::rng().fill_bytes(&mut random_bytes);
        let secret = format!("oc_secret_{}", URL_SAFE_NO_PAD.encode(random_bytes));

        let mut hasher = Sha256::new();
        hasher.update(secret.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        let encrypted = app_state.encryption_service.encrypt(&secret)?;
        Ok((Some(secret), Some(hash), Some(encrypted)))
    }
}

fn validate_optional_url(field_name: &str, value: Option<&str>) -> Result<(), AppError> {
    let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(());
    };

    let parsed = url::Url::parse(value)
        .map_err(|_| AppError::Validation(format!("{field_name} must be a valid URL")))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(AppError::Validation(format!(
            "{field_name} must use http or https"
        )));
    }

    Ok(())
}

fn validate_redirect_uris(redirect_uris: &[String], grant_types: &[String]) -> Result<(), AppError> {
    let requires_redirect = grant_types.iter().any(|g| g == "authorization_code");
    if requires_redirect && redirect_uris.is_empty() {
        return Err(AppError::Validation(
            "redirect_uris must include at least one URI for authorization_code".to_string(),
        ));
    }

    for uri in redirect_uris {
        let trimmed = uri.trim();
        if trimmed.is_empty() {
            return Err(AppError::Validation(
                "redirect_uris must not contain empty values".to_string(),
            ));
        }
        let parsed = url::Url::parse(trimmed)
            .map_err(|_| AppError::Validation("redirect_uris must contain valid URLs".to_string()))?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err(AppError::Validation(
                "redirect_uris must use http or https".to_string(),
            ));
        }
        if parsed.fragment().is_some() {
            return Err(AppError::Validation(
                "redirect_uris must not include fragments".to_string(),
            ));
        }
    }

    Ok(())
}

impl Command for CreateOAuthClientCommand {
    type Output = OAuthClientWithSecret;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.validate()?;

        let id = app_state.sf.next_id()? as i64;
        let client_id = Self::generate_client_id();
        let (client_secret, client_secret_hash, client_secret_encrypted) =
            self.generate_client_secret_hash_and_encrypted(app_state)?;
        let public_key_pem = self
            .public_key_pem
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let normalized_jwks = if let Some(pem) = &public_key_pem {
            Some(JwksDocument {
                keys: vec![models::api_key::Jwk {
                    kty: "PEM".to_string(),
                    kid: None,
                    use_: None,
                    key_ops: None,
                    alg: None,
                    n: None,
                    e: None,
                    crv: None,
                    x: None,
                    y: None,
                    k: None,
                    x5u: None,
                    x5c: None,
                    x5t: None,
                    x5t_s256: None,
                    public_key_pem: Some(pem.to_string()),
                }],
            })
        } else {
            self.jwks
        };
        let jwks_json = normalized_jwks
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;
        let token_endpoint_auth_signing_alg = self
            .token_endpoint_auth_signing_alg
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let client_name = self
            .client_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let client_uri = self
            .client_uri
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let logo_uri = self
            .logo_uri
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let tos_uri = self
            .tos_uri
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let policy_uri = self
            .policy_uri
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        validate_optional_url("client_uri", client_uri.as_deref())?;
        validate_optional_url("logo_uri", logo_uri.as_deref())?;
        validate_optional_url("tos_uri", tos_uri.as_deref())?;
        validate_optional_url("policy_uri", policy_uri.as_deref())?;
        let contacts_json = serde_json::to_value(self.contacts.unwrap_or_default())?;
        let software_id = self
            .software_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let software_version = self
            .software_version
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let jwks_uri = self
            .jwks_uri
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);

        let row = sqlx::query!(
            r#"
            INSERT INTO oauth_clients (
                id,
                deployment_id,
                oauth_app_id,
                client_id,
                client_secret_hash,
                client_secret_encrypted,
                client_auth_method,
                grant_types,
                redirect_uris,
                token_endpoint_auth_signing_alg,
                jwks_uri,
                jwks,
                client_name,
                client_uri,
                logo_uri,
                tos_uri,
                policy_uri,
                contacts,
                software_id,
                software_version,
                pkce_required,
                is_active
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,true,true)
            RETURNING
                id,
                deployment_id,
                oauth_app_id,
                client_id,
                client_auth_method,
                grant_types as "grant_types: serde_json::Value",
                redirect_uris as "redirect_uris: serde_json::Value",
                token_endpoint_auth_signing_alg,
                jwks_uri,
                jwks as "jwks: sqlx::types::Json<models::api_key::JwksDocument>",
                client_name,
                client_uri,
                logo_uri,
                tos_uri,
                policy_uri,
                contacts as "contacts: serde_json::Value",
                software_id,
                software_version,
                pkce_required,
                is_active,
                created_at,
                updated_at
            "#,
            id,
            self.deployment_id,
            self.oauth_app_id,
            client_id,
            client_secret_hash,
            client_secret_encrypted,
            self.client_auth_method,
            serde_json::to_value(&self.grant_types)?,
            serde_json::to_value(&self.redirect_uris)?,
            token_endpoint_auth_signing_alg,
            jwks_uri,
            jwks_json,
            client_name,
            client_uri,
            logo_uri,
            tos_uri,
            policy_uri,
            contacts_json,
            software_id,
            software_version
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        let jwks = row.jwks.map(|j| j.0);
        let stored_public_key_pem = jwks
            .as_ref()
            .and_then(models::api_key::JwksDocument::public_key_pem)
            .or(public_key_pem);

        Ok(OAuthClientWithSecret {
            client: OAuthClientData {
                id: row.id,
                deployment_id: row.deployment_id,
                oauth_app_id: row.oauth_app_id,
                client_id: row.client_id,
                client_auth_method: row.client_auth_method,
                grant_types: row.grant_types.clone(),
                redirect_uris: row.redirect_uris.clone(),
                token_endpoint_auth_signing_alg: row.token_endpoint_auth_signing_alg,
                jwks_uri: row.jwks_uri,
                jwks,
                public_key_pem: stored_public_key_pem,
                client_name: row.client_name,
                client_uri: row.client_uri,
                logo_uri: row.logo_uri,
                tos_uri: row.tos_uri,
                policy_uri: row.policy_uri,
                contacts: row.contacts,
                software_id: row.software_id,
                software_version: row.software_version,
                pkce_required: row.pkce_required,
                is_active: row.is_active,
                created_at: row.created_at,
                updated_at: row.updated_at,
            },
            client_secret,
        })
    }
}

pub struct CreateOAuthClientGrantCommand {
    pub deployment_id: i64,
    pub api_auth_app_slug: String,
    pub oauth_client_id: i64,
    pub resource: String,
    pub scopes: Vec<String>,
    pub granted_by_user_id: Option<i64>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct OAuthClientGrantCreated {
    pub id: i64,
}

impl Command for CreateOAuthClientGrantCommand {
    type Output = OAuthClientGrantCreated;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let resource = self.resource.trim();
        if resource.is_empty() {
            return Err(AppError::Validation("resource is required".to_string()));
        }
        if resource == "*" || resource.eq_ignore_ascii_case("all") {
            return Err(AppError::Validation(
                "wildcard/all resource grants are not allowed".to_string(),
            ));
        }
        let valid_resource = resource
            .strip_prefix("urn:wacht:organization:")
            .or_else(|| resource.strip_prefix("urn:wacht:workspace:"))
            .or_else(|| resource.strip_prefix("urn:wacht:user:"))
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|id| *id > 0)
            .is_some();
        if !valid_resource {
            return Err(AppError::Validation(
                "resource must be an absolute URI (e.g. urn:wacht:workspace:123)".to_string(),
            ));
        }
        let scope_policy = sqlx::query!(
            r#"
            SELECT oa.supported_scopes as "supported_scopes: serde_json::Value"
            FROM oauth_clients c
            INNER JOIN oauth_apps oa
              ON oa.id = c.oauth_app_id
             AND oa.deployment_id = c.deployment_id
            WHERE c.deployment_id = $1
              AND c.id = $2
            "#,
            self.deployment_id,
            self.oauth_client_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?
        .ok_or_else(|| AppError::NotFound("OAuth client not found".to_string()))?;

        let supported_scopes: Vec<String> =
            serde_json::from_value(scope_policy.supported_scopes).unwrap_or_default();
        if supported_scopes.is_empty() {
            return Err(AppError::Validation(
                "OAuth app has no supported scopes configured".to_string(),
            ));
        }

        let invalid_scopes: Vec<String> = self
            .scopes
            .iter()
            .filter(|scope| !supported_scopes.iter().any(|s| s == *scope))
            .cloned()
            .collect();
        if !invalid_scopes.is_empty() {
            return Err(AppError::Validation(format!(
                "Unsupported scopes for this OAuth app: {}",
                invalid_scopes.join(", ")
            )));
        }

        let id = app_state.sf.next_id()? as i64;
        let rec = sqlx::query!(
            r#"
            INSERT INTO oauth_client_grants (
                id,
                deployment_id,
                app_slug,
                oauth_client_id,
                resource,
                scopes,
                status,
                granted_at,
                expires_at,
                granted_by_user_id,
                created_at,
                updated_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,'active',NOW(),$7,$8,NOW(),NOW())
            RETURNING id
            "#,
            id,
            self.deployment_id,
            self.api_auth_app_slug,
            self.oauth_client_id,
            resource,
            serde_json::to_value(&self.scopes)?,
            self.expires_at,
            self.granted_by_user_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(OAuthClientGrantCreated { id: rec.id })
    }
}

pub struct RevokeOAuthClientGrantCommand {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub grant_id: i64,
}

impl Command for RevokeOAuthClientGrantCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!(
            r#"
            UPDATE oauth_client_grants
            SET
                status = 'revoked',
                revoked_at = NOW(),
                updated_at = NOW()
            WHERE deployment_id = $1
              AND oauth_client_id = $2
              AND id = $3
              AND status = 'active'
            "#,
            self.deployment_id,
            self.oauth_client_id,
            self.grant_id
        )
        .execute(&app_state.db_pool)
        .await?;

        Ok(())
    }
}

pub struct SetOAuthClientRegistrationAccessToken {
    pub oauth_app_id: i64,
    pub client_id: String,
    pub registration_access_token_hash: Option<String>,
}

impl Command for SetOAuthClientRegistrationAccessToken {
    type Output = bool;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let res = sqlx::query!(
            r#"
            UPDATE oauth_clients
            SET
                registration_access_token_hash = $3,
                updated_at = NOW()
            WHERE oauth_app_id = $1
              AND client_id = $2
            "#,
            self.oauth_app_id,
            self.client_id,
            self.registration_access_token_hash
        )
        .execute(&app_state.db_pool)
        .await?;

        Ok(res.rows_affected() > 0)
    }
}

pub struct DeactivateOAuthClient {
    pub oauth_app_id: i64,
    pub client_id: String,
}

impl Command for DeactivateOAuthClient {
    type Output = bool;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let res = sqlx::query!(
            r#"
            UPDATE oauth_clients
            SET
                is_active = FALSE,
                registration_access_token_hash = NULL,
                updated_at = NOW()
            WHERE oauth_app_id = $1
              AND client_id = $2
              AND is_active = TRUE
            "#,
            self.oauth_app_id,
            self.client_id
        )
        .execute(&app_state.db_pool)
        .await?;

        Ok(res.rows_affected() > 0)
    }
}

pub struct UpdateOAuthClientSettings {
    pub oauth_app_id: i64,
    pub client_id: String,
    pub client_auth_method: Option<String>,
    pub grant_types: Option<Vec<String>>,
    pub redirect_uris: Option<Vec<String>>,
    pub token_endpoint_auth_signing_alg: Option<String>,
    pub jwks_uri: Option<String>,
    pub jwks: Option<JwksDocument>,
    pub public_key_pem: Option<String>,
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub logo_uri: Option<String>,
    pub tos_uri: Option<String>,
    pub policy_uri: Option<String>,
    pub contacts: Option<Vec<String>>,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
}

impl Command for UpdateOAuthClientSettings {
    type Output = Option<OAuthClientData>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        if let Some(grant_types) = &self.grant_types {
            if grant_types.is_empty() {
                return Err(AppError::Validation(
                    "grant_types must include at least one grant".to_string(),
                ));
            }
            if grant_types.iter().any(|g| g == "client_credentials") {
                return Err(AppError::Validation(
                    "client_credentials is disabled for now".to_string(),
                ));
            }
        }

        let current = sqlx::query!(
            r#"
            SELECT
                id,
                deployment_id,
                oauth_app_id,
                client_id,
                client_auth_method,
                grant_types as "grant_types: serde_json::Value",
                redirect_uris as "redirect_uris: serde_json::Value",
                token_endpoint_auth_signing_alg,
                jwks_uri,
                jwks as "jwks: sqlx::types::Json<models::api_key::JwksDocument>",
                client_name,
                client_uri,
                logo_uri,
                tos_uri,
                policy_uri,
                contacts as "contacts: serde_json::Value",
                software_id,
                software_version,
                pkce_required,
                is_active,
                created_at,
                updated_at
            FROM oauth_clients
            WHERE oauth_app_id = $1
              AND client_id = $2
              AND is_active = TRUE
            "#,
            self.oauth_app_id,
            self.client_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        let Some(current) = current else {
            return Ok(None);
        };

        let effective_method = self
            .client_auth_method
            .clone()
            .unwrap_or_else(|| current.client_auth_method.clone());
        let effective_grant_types = self
            .grant_types
            .clone()
            .unwrap_or_else(|| serde_json::from_value(current.grant_types.clone()).unwrap_or_default());
        let effective_redirect_uris = self
            .redirect_uris
            .clone()
            .unwrap_or_else(|| serde_json::from_value(current.redirect_uris.clone()).unwrap_or_default());
        let allowed = [
            "client_secret_basic",
            "client_secret_post",
            "client_secret_jwt",
            "none",
            "private_key_jwt",
        ];
        if !allowed.contains(&effective_method.as_str()) {
            return Err(AppError::Validation(format!(
                "unsupported client_auth_method: {}",
                effective_method
            )));
        }
        validate_redirect_uris(&effective_redirect_uris, &effective_grant_types)?;

        let token_endpoint_auth_signing_alg = self
            .token_endpoint_auth_signing_alg
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let jwks_uri = self
            .jwks_uri
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let public_key_pem = self
            .public_key_pem
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let client_name = self
            .client_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let client_uri = self
            .client_uri
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let logo_uri = self
            .logo_uri
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let tos_uri = self
            .tos_uri
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let policy_uri = self
            .policy_uri
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let contacts_json = self
            .contacts
            .filter(|items| !items.is_empty())
            .map(|items| serde_json::to_value(items))
            .transpose()?;
        let software_id = self
            .software_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let software_version = self
            .software_version
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);

        let has_jwks_uri = jwks_uri.is_some();
        let has_jwks = self.jwks.is_some();
        let has_public_key_pem = public_key_pem.is_some();
        if effective_method == "private_key_jwt" {
            let key_material_count = has_jwks_uri as u8 + has_jwks as u8 + has_public_key_pem as u8;
            if key_material_count > 1 {
                return Err(AppError::Validation(
                    "private_key_jwt accepts only one of jwks_uri, jwks, or public_key_pem per update"
                        .to_string(),
                ));
            }
            if key_material_count == 0 && current.jwks_uri.is_none() && current.jwks.is_none() {
                return Err(AppError::Validation(
                    "private_key_jwt requires existing or new key material".to_string(),
                ));
            }

            if let Some(uri) = &jwks_uri {
                let parsed = url::Url::parse(uri).map_err(|_| {
                    AppError::Validation("jwks_uri must be a valid URL".to_string())
                })?;
                if parsed.scheme() != "https" {
                    return Err(AppError::Validation("jwks_uri must use https".to_string()));
                }
            }
            if let Some(jwks) = &self.jwks {
                CreateOAuthClientCommand::validate_jwks_document(jwks)?;
            }
        } else if has_jwks_uri || has_jwks || has_public_key_pem {
            return Err(AppError::Validation(
                "jwks_uri/jwks/public_key_pem are only allowed when client_auth_method is private_key_jwt".to_string(),
            ));
        }

        let normalized_jwks = if let Some(pem) = &public_key_pem {
            Some(JwksDocument {
                keys: vec![models::api_key::Jwk {
                    kty: "PEM".to_string(),
                    kid: None,
                    use_: None,
                    key_ops: None,
                    alg: None,
                    n: None,
                    e: None,
                    crv: None,
                    x: None,
                    y: None,
                    k: None,
                    x5u: None,
                    x5c: None,
                    x5t: None,
                    x5t_s256: None,
                    public_key_pem: Some(pem.to_string()),
                }],
            })
        } else {
            self.jwks
        };
        let grant_types_json = self
            .grant_types
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;
        let redirect_uris_json = self
            .redirect_uris
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;
        let jwks_json = normalized_jwks
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;

        let row = sqlx::query!(
            r#"
            UPDATE oauth_clients
            SET
                client_auth_method = COALESCE($3, client_auth_method),
                grant_types = COALESCE($4, grant_types),
                redirect_uris = COALESCE($5, redirect_uris),
                token_endpoint_auth_signing_alg = CASE
                    WHEN $6::text IS NULL THEN token_endpoint_auth_signing_alg
                    ELSE $6
                END,
                jwks_uri = CASE
                    WHEN $7::text IS NULL THEN jwks_uri
                    ELSE $7
                END,
                jwks = CASE
                    WHEN $8::jsonb IS NULL THEN jwks
                    ELSE $8
                END,
                client_name = CASE
                    WHEN $9::text IS NULL THEN client_name
                    ELSE $9
                END,
                client_uri = CASE
                    WHEN $10::text IS NULL THEN client_uri
                    ELSE $10
                END,
                logo_uri = CASE
                    WHEN $11::text IS NULL THEN logo_uri
                    ELSE $11
                END,
                tos_uri = CASE
                    WHEN $12::text IS NULL THEN tos_uri
                    ELSE $12
                END,
                policy_uri = CASE
                    WHEN $13::text IS NULL THEN policy_uri
                    ELSE $13
                END,
                contacts = CASE
                    WHEN $14::jsonb IS NULL THEN contacts
                    ELSE $14
                END,
                software_id = CASE
                    WHEN $15::text IS NULL THEN software_id
                    ELSE $15
                END,
                software_version = CASE
                    WHEN $16::text IS NULL THEN software_version
                    ELSE $16
                END,
                updated_at = NOW()
            WHERE oauth_app_id = $1
              AND client_id = $2
              AND is_active = TRUE
            RETURNING
                id,
                deployment_id,
                oauth_app_id,
                client_id,
                client_auth_method,
                grant_types as "grant_types: serde_json::Value",
                redirect_uris as "redirect_uris: serde_json::Value",
                token_endpoint_auth_signing_alg,
                jwks_uri,
                jwks as "jwks: sqlx::types::Json<models::api_key::JwksDocument>",
                client_name,
                client_uri,
                logo_uri,
                tos_uri,
                policy_uri,
                contacts as "contacts: serde_json::Value",
                software_id,
                software_version,
                pkce_required,
                is_active,
                created_at,
                updated_at
            "#,
            self.oauth_app_id,
            self.client_id,
            self.client_auth_method,
            grant_types_json,
            redirect_uris_json,
            token_endpoint_auth_signing_alg,
            jwks_uri,
            jwks_json,
            client_name,
            client_uri,
            logo_uri,
            tos_uri,
            policy_uri,
            contacts_json,
            software_id,
            software_version
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(row.map(|r| {
            let jwks = r.jwks.map(|j| j.0);
            let public_key_pem = jwks
                .as_ref()
                .and_then(models::api_key::JwksDocument::public_key_pem);
            OAuthClientData {
                id: r.id,
                deployment_id: r.deployment_id,
                oauth_app_id: r.oauth_app_id,
                client_id: r.client_id,
                client_auth_method: r.client_auth_method,
                grant_types: r.grant_types,
                redirect_uris: r.redirect_uris,
                token_endpoint_auth_signing_alg: r.token_endpoint_auth_signing_alg,
                jwks_uri: r.jwks_uri,
                jwks,
                public_key_pem,
                client_name: r.client_name,
                client_uri: r.client_uri,
                logo_uri: r.logo_uri,
                tos_uri: r.tos_uri,
                policy_uri: r.policy_uri,
                contacts: r.contacts,
                software_id: r.software_id,
                software_version: r.software_version,
                pkce_required: r.pkce_required,
                is_active: r.is_active,
                created_at: r.created_at,
                updated_at: r.updated_at,
            }
        }))
    }
}

pub struct RotateOAuthClientSecret {
    pub oauth_app_id: i64,
    pub client_id: String,
}

impl RotateOAuthClientSecret {
    fn generate_client_secret_hash_and_encrypted(
        app_state: &AppState,
    ) -> Result<(String, String, String), AppError> {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        use rand::RngCore;
        let mut random_bytes = vec![0u8; 32];
        rand::rng().fill_bytes(&mut random_bytes);
        let secret = format!("oc_secret_{}", URL_SAFE_NO_PAD.encode(random_bytes));

        let mut hasher = Sha256::new();
        hasher.update(secret.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        let encrypted = app_state.encryption_service.encrypt(&secret)?;
        Ok((secret, hash, encrypted))
    }
}

impl Command for RotateOAuthClientSecret {
    type Output = Option<String>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let client = sqlx::query!(
            r#"
            SELECT client_auth_method
            FROM oauth_clients
            WHERE oauth_app_id = $1
              AND client_id = $2
              AND is_active = TRUE
            "#,
            self.oauth_app_id,
            self.client_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        let Some(client) = client else {
            return Ok(None);
        };

        if client.client_auth_method == "none" || client.client_auth_method == "private_key_jwt" {
            return Err(AppError::Validation(
                "client_secret rotation is not supported for this auth method".to_string(),
            ));
        }

        let (client_secret, client_secret_hash, client_secret_encrypted) =
            Self::generate_client_secret_hash_and_encrypted(app_state)?;
        sqlx::query!(
            r#"
            UPDATE oauth_clients
            SET
                client_secret_hash = $3,
                client_secret_encrypted = $4,
                updated_at = NOW()
            WHERE oauth_app_id = $1
              AND client_id = $2
              AND is_active = TRUE
            "#,
            self.oauth_app_id,
            self.client_id,
            client_secret_hash,
            client_secret_encrypted
        )
        .execute(&app_state.db_pool)
        .await?;

        Ok(Some(client_secret))
    }
}
