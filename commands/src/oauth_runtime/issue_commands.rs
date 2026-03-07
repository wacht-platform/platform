use chrono::{Duration, Utc};
use common::error::AppError;

use super::helpers::{generate_token, hash_value};

pub struct IssueOAuthAuthorizationCode {
    pub code_id: Option<i64>,
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub oauth_grant_id: i64,
    pub app_slug: String,
    pub redirect_uri: String,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub scopes: Vec<String>,
    pub resource: Option<String>,
    pub granted_resource: Option<String>,
}

pub struct OAuthAuthorizationCodeIssued {
    pub code: String,
    pub expires_in: i64,
}

impl IssueOAuthAuthorizationCode {
    pub fn with_code_id(mut self, code_id: i64) -> Self {
        self.code_id = Some(code_id);
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<OAuthAuthorizationCodeIssued, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let code_id = self
            .code_id
            .ok_or_else(|| AppError::Validation("code_id is required".to_string()))?;
        let code = generate_token("oac", 32);
        let code_hash = hash_value(&code);
        let expires_at = Utc::now() + Duration::minutes(10);

        sqlx::query(
            r#"
            INSERT INTO oauth_authorization_codes (
                id,
                deployment_id,
                oauth_client_id,
                oauth_grant_id,
                app_slug,
                code_hash,
                redirect_uri,
                pkce_code_challenge,
                pkce_code_challenge_method,
                scopes,
                resource,
                granted_resource,
                expires_at,
                created_at
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,NOW())
            "#,
        )
        .bind(code_id)
        .bind(self.deployment_id)
        .bind(self.oauth_client_id)
        .bind(self.oauth_grant_id)
        .bind(&self.app_slug)
        .bind(code_hash)
        .bind(&self.redirect_uri)
        .bind(&self.code_challenge)
        .bind(&self.code_challenge_method)
        .bind(serde_json::to_value(&self.scopes)?)
        .bind(&self.resource)
        .bind(&self.granted_resource)
        .bind(expires_at)
        .execute(executor)
        .await?;

        Ok(OAuthAuthorizationCodeIssued {
            code,
            expires_in: 600,
        })
    }
}

pub struct ConsumeOAuthAuthorizationCode {
    pub code_id: i64,
}

impl ConsumeOAuthAuthorizationCode {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<bool, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let res = sqlx::query(
            r#"
            UPDATE oauth_authorization_codes
            SET consumed_at = NOW()
            WHERE id = $1
              AND consumed_at IS NULL
            "#,
        )
        .bind(self.code_id)
        .execute(executor)
        .await?;
        Ok(res.rows_affected() > 0)
    }
}

pub struct IssueOAuthTokenPair {
    pub access_token_id: Option<i64>,
    pub refresh_token_id: Option<i64>,
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub oauth_grant_id: i64,
    pub app_slug: String,
    pub scopes: Vec<String>,
    pub resource: Option<String>,
    pub granted_resource: Option<String>,
}

pub struct OAuthTokenPairIssued {
    pub access_token: String,
    pub refresh_token: String,
    pub refresh_token_id: i64,
    pub access_expires_in: i64,
}

impl IssueOAuthTokenPair {
    pub fn with_access_token_id(mut self, access_token_id: i64) -> Self {
        self.access_token_id = Some(access_token_id);
        self
    }

    pub fn with_refresh_token_id(mut self, refresh_token_id: i64) -> Self {
        self.refresh_token_id = Some(refresh_token_id);
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<OAuthTokenPairIssued, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let access_token_id = self
            .access_token_id
            .ok_or_else(|| AppError::Validation("access_token_id is required".to_string()))?;
        let refresh_token_id = self
            .refresh_token_id
            .ok_or_else(|| AppError::Validation("refresh_token_id is required".to_string()))?;
        let access_token = generate_token("oat", 32);
        let refresh_token = generate_token("ort", 32);
        let access_hash = hash_value(&access_token);
        let refresh_hash = hash_value(&refresh_token);

        sqlx::query(
            r#"
            WITH inserted_access AS (
                INSERT INTO oauth_access_tokens (
                    id,
                    deployment_id,
                    oauth_client_id,
                    oauth_grant_id,
                    app_slug,
                    token_hash,
                    principal_type,
                    scopes,
                    resource,
                    granted_resource,
                    expires_at,
                    created_at
                ) VALUES ($1,$2,$3,$4,$5,$6,'user_oauth',$7,$8,$9,$10,NOW())
            )
            INSERT INTO oauth_refresh_tokens (
                id,
                deployment_id,
                oauth_client_id,
                oauth_grant_id,
                app_slug,
                token_hash,
                scopes,
                resource,
                granted_resource,
                expires_at,
                created_at
            ) VALUES ($11,$2,$3,$4,$5,$12,$7,$8,$9,$13,NOW())
            "#,
        )
        .bind(access_token_id)
        .bind(self.deployment_id)
        .bind(self.oauth_client_id)
        .bind(self.oauth_grant_id)
        .bind(&self.app_slug)
        .bind(access_hash)
        .bind(serde_json::to_value(&self.scopes)?)
        .bind(&self.resource)
        .bind(&self.granted_resource)
        .bind(Utc::now() + Duration::hours(1))
        .bind(refresh_token_id)
        .bind(refresh_hash)
        .bind(Utc::now() + Duration::days(30))
        .execute(executor)
        .await?;

        Ok(OAuthTokenPairIssued {
            access_token,
            refresh_token,
            refresh_token_id,
            access_expires_in: 3600,
        })
    }
}
