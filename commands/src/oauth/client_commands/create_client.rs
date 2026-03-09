use super::*;

pub struct CreateOAuthClientCommand {
    pub client_record_id: Option<i64>,
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
                validate_jwks_document(jwks)?;
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

    pub fn with_client_record_id(mut self, client_record_id: i64) -> Self {
        self.client_record_id = Some(client_record_id);
        self
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<OAuthClientWithSecret, AppError>
    where
        D: HasDbRouter + HasEncryptionProvider,
    {
        let writer = deps.db_router().writer();
        let client_record_id = self
            .client_record_id
            .ok_or_else(|| AppError::Validation("client_record_id is required".to_string()))?;
        let encryptor = deps.encryption_provider();
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
        .fetch_one(writer)
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
