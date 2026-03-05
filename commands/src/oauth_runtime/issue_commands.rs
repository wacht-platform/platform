use crate::Command;
use chrono::{Duration, Utc};
use common::error::AppError;
use common::state::AppState;

use super::helpers::{generate_token, hash_value};

pub struct IssueOAuthAuthorizationCode {
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

impl Command for IssueOAuthAuthorizationCode {
    type Output = OAuthAuthorizationCodeIssued;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let code = generate_token("oac", 32);
        let id = app_state.sf.next_id()? as i64;
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
        .bind(id)
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
        .execute(&app_state.db_pool)
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

impl Command for ConsumeOAuthAuthorizationCode {
    type Output = bool;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let res = sqlx::query(
            r#"
            UPDATE oauth_authorization_codes
            SET consumed_at = NOW()
            WHERE id = $1
              AND consumed_at IS NULL
            "#,
        )
        .bind(self.code_id)
        .execute(&app_state.db_pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }
}

pub struct IssueOAuthTokenPair {
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

impl Command for IssueOAuthTokenPair {
    type Output = OAuthTokenPairIssued;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let access_token = generate_token("oat", 32);
        let refresh_token = generate_token("ort", 32);
        let access_hash = hash_value(&access_token);
        let refresh_hash = hash_value(&refresh_token);
        let access_id = app_state.sf.next_id()? as i64;
        let refresh_id = app_state.sf.next_id()? as i64;

        sqlx::query(
            r#"
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
            "#,
        )
        .bind(access_id)
        .bind(self.deployment_id)
        .bind(self.oauth_client_id)
        .bind(self.oauth_grant_id)
        .bind(&self.app_slug)
        .bind(access_hash)
        .bind(serde_json::to_value(&self.scopes)?)
        .bind(&self.resource)
        .bind(&self.granted_resource)
        .bind(Utc::now() + Duration::hours(1))
        .execute(&app_state.db_pool)
        .await?;

        sqlx::query(
            r#"
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
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,NOW())
            "#,
        )
        .bind(refresh_id)
        .bind(self.deployment_id)
        .bind(self.oauth_client_id)
        .bind(self.oauth_grant_id)
        .bind(&self.app_slug)
        .bind(refresh_hash)
        .bind(serde_json::to_value(&self.scopes)?)
        .bind(&self.resource)
        .bind(&self.granted_resource)
        .bind(Utc::now() + Duration::days(30))
        .execute(&app_state.db_pool)
        .await?;

        Ok(OAuthTokenPairIssued {
            access_token,
            refresh_token,
            refresh_token_id: refresh_id,
            access_expires_in: 3600,
        })
    }
}
