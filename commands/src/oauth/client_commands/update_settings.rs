use super::*;

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

impl UpdateOAuthClientSettings {
    pub async fn execute_with_db<'a, Db>(
        self,
        db: Db,
    ) -> Result<Option<OAuthClientData>, AppError>
    where
        Db: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = db.begin().await?;
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
        .fetch_optional(&mut *tx)
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
            .unwrap_or_else(|| json_default(current.grant_types.clone()));
        let effective_redirect_uris = self
            .redirect_uris
            .clone()
            .unwrap_or_else(|| json_default(current.redirect_uris.clone()));
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
            .map(serde_json::to_value)
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
                validate_jwks_document(jwks)?;
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
        .fetch_optional(&mut *tx)
        .await?;

        tx.commit().await?;

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
