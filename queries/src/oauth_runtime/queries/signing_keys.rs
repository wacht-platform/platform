//! Per-OAuth-app RSA signing keys. Private PEM is loaded only by the signing
//! path (`GetOAuthAppActiveSigningKeyQuery`); all other callers get
//! `OAuthAppPublishableKey`, which omits it.

use common::error::AppError;

#[derive(Debug, Clone)]
pub struct OAuthAppSigningKey {
    pub kid: String,
    pub algorithm: String,
    pub public_key_pem: String,
    pub private_key_pem: String,
    pub status: String,
}

/// Public-only projection — used everywhere except the actual signing call.
#[derive(Debug, Clone)]
pub struct OAuthAppPublishableKey {
    pub kid: String,
    pub algorithm: String,
    pub public_key_pem: String,
    pub status: String,
}

impl From<OAuthAppSigningKey> for OAuthAppPublishableKey {
    fn from(key: OAuthAppSigningKey) -> Self {
        Self {
            kid: key.kid,
            algorithm: key.algorithm,
            public_key_pem: key.public_key_pem,
            status: key.status,
        }
    }
}

/// Active + retired keys for JWKS / logout verification — public-only.
pub struct ListOAuthAppPublishableSigningKeysQuery {
    pub oauth_app_id: i64,
}

impl ListOAuthAppPublishableSigningKeysQuery {
    pub fn new(oauth_app_id: i64) -> Self {
        Self { oauth_app_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<OAuthAppPublishableKey>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            r#"
            SELECT kid, algorithm, public_key_pem, status
              FROM oauth_app_signing_keys
             WHERE oauth_app_id = $1
               AND status IN ('active', 'retired')
             ORDER BY activated_at DESC
            "#,
            self.oauth_app_id
        )
        .fetch_all(executor)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| OAuthAppPublishableKey {
                kid: r.kid,
                algorithm: r.algorithm,
                public_key_pem: r.public_key_pem,
                status: r.status,
            })
            .collect())
    }
}

/// Currently-active signing key (loads the private PEM — signing path only).
pub struct GetOAuthAppActiveSigningKeyQuery {
    pub oauth_app_id: i64,
}

impl GetOAuthAppActiveSigningKeyQuery {
    pub fn new(oauth_app_id: i64) -> Self {
        Self { oauth_app_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<OAuthAppSigningKey>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT kid, algorithm, public_key_pem, private_key_pem, status
              FROM oauth_app_signing_keys
             WHERE oauth_app_id = $1
               AND status = 'active'
             LIMIT 1
            "#,
            self.oauth_app_id
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| OAuthAppSigningKey {
            kid: r.kid,
            algorithm: r.algorithm,
            public_key_pem: r.public_key_pem,
            private_key_pem: r.private_key_pem,
            status: r.status,
        }))
    }
}
