use crate::Command;
use common::{EncryptionService, HasDbRouter, HasEncryptionService, error::AppError};
use common::state::AppState;
use models::api_key::JwksDocument;
use queries::oauth::OAuthClientData;
use sha2::{Digest, Sha256};

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

pub trait OAuthClientSecretEncryptor: Send + Sync {
    fn encrypt(&self, plaintext: &str) -> Result<String, AppError>;
}

impl OAuthClientSecretEncryptor for EncryptionService {
    fn encrypt(&self, plaintext: &str) -> Result<String, AppError> {
        EncryptionService::encrypt(self, plaintext)
    }
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
        validate_oauth_client_grant_types(&self.grant_types)?;
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
        encryptor: &dyn OAuthClientSecretEncryptor,
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
        let encrypted = encryptor.encrypt(&secret)?;
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

fn validate_redirect_uris(
    redirect_uris: &[String],
    grant_types: &[String],
) -> Result<(), AppError> {
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
        let parsed = url::Url::parse(trimmed).map_err(|_| {
            AppError::Validation("redirect_uris must contain valid URLs".to_string())
        })?;
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
        self.execute_with(
            app_state.db_router.writer(),
            &app_state.encryption_service,
            app_state.sf.next_id()? as i64,
        )
        .await
    }
}

impl CreateOAuthClientCommand {
    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        encryptor: &dyn OAuthClientSecretEncryptor,
        client_record_id: i64,
    ) -> Result<OAuthClientWithSecret, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_deps(conn, encryptor, client_record_id)
            .await
    }

    async fn execute_with_deps<C>(
        self,
        mut conn: C,
        encryptor: &dyn OAuthClientSecretEncryptor,
        client_record_id: i64,
    ) -> Result<OAuthClientWithSecret, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
        self.validate()?;

        let client_id = Self::generate_client_id();
        let (client_secret, client_secret_hash, client_secret_encrypted) =
            self.generate_client_secret_hash_and_encrypted(encryptor)?;
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
            client_record_id,
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
        .fetch_one(&mut *conn)
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

pub struct SetOAuthClientRegistrationAccessToken {
    pub oauth_app_id: i64,
    pub client_id: String,
    pub registration_access_token_hash: Option<String>,
}

impl Command for SetOAuthClientRegistrationAccessToken {
    type Output = bool;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}

impl SetOAuthClientRegistrationAccessToken {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<bool, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_deps(conn).await
    }

    async fn execute_with_deps<C>(self, mut conn: C) -> Result<bool, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
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
        .execute(&mut *conn)
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
        self.execute_with(app_state.db_router.writer()).await
    }
}

impl DeactivateOAuthClient {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<bool, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_deps(conn).await
    }

    async fn execute_with_deps<C>(self, mut conn: C) -> Result<bool, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
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
        .execute(&mut *conn)
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
        self.execute_with(app_state.db_router.writer()).await
    }
}

impl UpdateOAuthClientSettings {
    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
    ) -> Result<Option<OAuthClientData>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_deps(conn).await
    }

    async fn execute_with_deps<C>(
        self,
        mut conn: C,
    ) -> Result<Option<OAuthClientData>, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
        if let Some(grant_types) = &self.grant_types {
            validate_oauth_client_grant_types(grant_types)?;
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
        .fetch_optional(&mut *conn)
        .await?;

        let Some(current) = current else {
            return Ok(None);
        };

        let effective_method = self
            .client_auth_method
            .clone()
            .unwrap_or_else(|| current.client_auth_method.clone());
        let effective_grant_types = self.grant_types.clone().unwrap_or_else(|| {
            serde_json::from_value(current.grant_types.clone()).unwrap_or_default()
        });
        let effective_redirect_uris = self.redirect_uris.clone().unwrap_or_else(|| {
            serde_json::from_value(current.redirect_uris.clone()).unwrap_or_default()
        });
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
        .fetch_optional(&mut *conn)
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

fn validate_oauth_client_grant_types(grant_types: &[String]) -> Result<(), AppError> {
    if grant_types.is_empty() {
        return Err(AppError::Validation(
            "grant_types must include at least one grant".to_string(),
        ));
    }

    let mut has_authorization_code = false;

    for grant in grant_types {
        let value = grant.trim();
        if value.is_empty() {
            return Err(AppError::Validation(
                "grant_types must not contain empty values".to_string(),
            ));
        }

        match value {
            "authorization_code" => {
                has_authorization_code = true;
            }
            "refresh_token" => {}
            "client_credentials" => {
                return Err(AppError::Validation(
                    "client_credentials is disabled for now".to_string(),
                ));
            }
            _ => {
                return Err(AppError::Validation(format!(
                    "unsupported grant_type: {}. supported values: authorization_code, refresh_token",
                    value
                )));
            }
        }
    }

    if !has_authorization_code {
        return Err(AppError::Validation(
            "authorization_code grant is required".to_string(),
        ));
    }

    Ok(())
}

pub struct RotateOAuthClientSecret {
    pub oauth_app_id: i64,
    pub client_id: String,
}

impl RotateOAuthClientSecret {
    fn generate_client_secret_hash_and_encrypted(
        encryptor: &dyn OAuthClientSecretEncryptor,
    ) -> Result<(String, String, String), AppError> {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        use rand::RngCore;
        let mut random_bytes = vec![0u8; 32];
        rand::rng().fill_bytes(&mut random_bytes);
        let secret = format!("oc_secret_{}", URL_SAFE_NO_PAD.encode(random_bytes));

        let mut hasher = Sha256::new();
        hasher.update(secret.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        let encrypted = encryptor.encrypt(&secret)?;
        Ok((secret, hash, encrypted))
    }
}

impl Command for RotateOAuthClientSecret {
    type Output = Option<String>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with_deps(app_state).await
    }
}

impl RotateOAuthClientSecret {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Option<String>, AppError>
    where
        D: HasDbRouter + HasEncryptionService,
    {
        self.execute_with(deps.db_router().writer(), deps.encryption_service())
            .await
    }

    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        encryptor: &dyn OAuthClientSecretEncryptor,
    ) -> Result<Option<String>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.apply_with_conn(conn, encryptor).await
    }

    async fn apply_with_conn<C>(
        self,
        mut conn: C,
        encryptor: &dyn OAuthClientSecretEncryptor,
    ) -> Result<Option<String>, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
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
        .fetch_optional(&mut *conn)
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
            Self::generate_client_secret_hash_and_encrypted(encryptor)?;
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
        .execute(&mut *conn)
        .await?;

        Ok(Some(client_secret))
    }
}
