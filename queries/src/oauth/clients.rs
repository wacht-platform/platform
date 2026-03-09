use super::*;

pub struct ListOAuthClientsByOAuthAppQuery {
    pub deployment_id: i64,
    pub oauth_app_id: i64,
}

impl ListOAuthClientsByOAuthAppQuery {
    pub fn new(deployment_id: i64, oauth_app_id: i64) -> Self {
        Self {
            deployment_id,
            oauth_app_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<OAuthClientData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
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
            WHERE deployment_id = $1
              AND oauth_app_id = $2
            ORDER BY created_at DESC
            "#,
            self.deployment_id,
            self.oauth_app_id
        )
        .fetch_all(executor)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let jwks = r.jwks.map(|j| j.0);
                let public_key_pem = jwks.as_ref().and_then(JwksDocument::public_key_pem);
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
            })
            .collect())
    }
}

pub struct GetOAuthClientByIdQuery {
    pub deployment_id: i64,
    pub oauth_app_id: i64,
    pub oauth_client_id: i64,
}

impl GetOAuthClientByIdQuery {
    pub fn new(deployment_id: i64, oauth_app_id: i64, oauth_client_id: i64) -> Self {
        Self {
            deployment_id,
            oauth_app_id,
            oauth_client_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<OAuthClientData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
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
            WHERE deployment_id = $1
              AND oauth_app_id = $2
              AND id = $3
            "#,
            self.deployment_id,
            self.oauth_app_id,
            self.oauth_client_id
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| {
            let jwks = r.jwks.map(|j| j.0);
            let public_key_pem = jwks.as_ref().and_then(JwksDocument::public_key_pem);
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
