use crate::Command;
use chrono::{Duration, Utc};
use common::error::AppError;
use common::state::AppState;
use rand::RngCore;
use redis::AsyncCommands;
use sha2::{Digest, Sha256};

const OAUTH_GRANT_LAST_USED_DIRTY_KEY: &str = "oauth:grant:last_used:dirty";

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
                expires_at,
                created_at
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,NOW())
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
                expires_at,
                created_at
            ) VALUES ($1,$2,$3,$4,$5,$6,'user_oauth',$7,$8,$9,NOW())
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
                expires_at,
                created_at
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,NOW())
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

pub struct RevokeOAuthRefreshTokenById {
    pub refresh_token_id: i64,
}

impl Command for RevokeOAuthRefreshTokenById {
    type Output = bool;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let res = sqlx::query(
            r#"
            UPDATE oauth_refresh_tokens
            SET revoked_at = NOW()
            WHERE id = $1
              AND revoked_at IS NULL
            "#,
        )
        .bind(self.refresh_token_id)
        .execute(&app_state.db_pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }
}

pub struct SetOAuthRefreshTokenReplacement {
    pub old_refresh_token_id: i64,
    pub new_refresh_token_id: i64,
}

impl Command for SetOAuthRefreshTokenReplacement {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query(
            r#"
            UPDATE oauth_refresh_tokens
            SET replaced_by_token_id = $2
            WHERE id = $1
            "#,
        )
        .bind(self.old_refresh_token_id)
        .bind(self.new_refresh_token_id)
        .execute(&app_state.db_pool)
        .await?;
        Ok(())
    }
}

pub struct RevokeOAuthAccessTokenByHash {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub token_hash: String,
}

impl Command for RevokeOAuthAccessTokenByHash {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query(
            r#"
            UPDATE oauth_access_tokens
            SET revoked_at = NOW()
            WHERE deployment_id = $1
              AND oauth_client_id = $2
              AND token_hash = $3
              AND revoked_at IS NULL
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.oauth_client_id)
        .bind(self.token_hash)
        .execute(&app_state.db_pool)
        .await?;
        Ok(())
    }
}

pub struct RevokeOAuthRefreshTokenByHash {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub token_hash: String,
}

impl Command for RevokeOAuthRefreshTokenByHash {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query(
            r#"
            UPDATE oauth_refresh_tokens
            SET revoked_at = NOW()
            WHERE deployment_id = $1
              AND oauth_client_id = $2
              AND token_hash = $3
              AND revoked_at IS NULL
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.oauth_client_id)
        .bind(self.token_hash)
        .execute(&app_state.db_pool)
        .await?;
        Ok(())
    }
}

pub struct EnqueueOAuthGrantLastUsed {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub grant_id: i64,
}

impl Command for EnqueueOAuthGrantLastUsed {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut redis_conn = app_state
            .redis_client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to connect redis: {}", e)))?;

        let member = format!(
            "{}:{}:{}",
            self.deployment_id, self.oauth_client_id, self.grant_id
        );
        let score = Utc::now().timestamp_millis() as f64;
        let _: () = redis_conn
            .zadd(OAUTH_GRANT_LAST_USED_DIRTY_KEY, member, score)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to enqueue grant usage: {}", e)))?;
        let _: bool = redis_conn
            .expire(OAUTH_GRANT_LAST_USED_DIRTY_KEY, 604800)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to set dirty-key expiry: {}", e)))?;

        Ok(())
    }
}

pub struct SyncOAuthGrantLastUsedBatch {
    pub batch_size: usize,
}

impl Command for SyncOAuthGrantLastUsedBatch {
    type Output = usize;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let batch_size = self.batch_size.max(1);

        let mut redis_conn = app_state
            .redis_client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to connect redis: {}", e)))?;

        let popped: Vec<(String, f64)> = redis::cmd("ZPOPMIN")
            .arg(OAUTH_GRANT_LAST_USED_DIRTY_KEY)
            .arg(batch_size)
            .query_async(&mut redis_conn)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to pop dirty grants: {}", e)))?;

        if popped.is_empty() {
            return Ok(0);
        }

        let mut deployment_ids = Vec::with_capacity(popped.len());
        let mut client_ids = Vec::with_capacity(popped.len());
        let mut grant_ids = Vec::with_capacity(popped.len());
        let mut used_ats = Vec::with_capacity(popped.len());

        for (member, score) in popped {
            let mut parts = member.split(':');
            let deployment_id = parts.next().and_then(|p| p.parse::<i64>().ok());
            let oauth_client_id = parts.next().and_then(|p| p.parse::<i64>().ok());
            let grant_id = parts.next().and_then(|p| p.parse::<i64>().ok());
            if deployment_id.is_none()
                || oauth_client_id.is_none()
                || grant_id.is_none()
                || parts.next().is_some()
            {
                continue;
            }
            let Some(used_at) = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(score as i64)
            else {
                continue;
            };
            deployment_ids.push(deployment_id.unwrap_or_default());
            client_ids.push(oauth_client_id.unwrap_or_default());
            grant_ids.push(grant_id.unwrap_or_default());
            used_ats.push(used_at);
        }

        if deployment_ids.is_empty() {
            return Ok(0);
        }

        let synced = grant_ids.len();

        sqlx::query(
            r#"
            WITH input AS (
                SELECT
                    UNNEST($1::bigint[]) AS deployment_id,
                    UNNEST($2::bigint[]) AS oauth_client_id,
                    UNNEST($3::bigint[]) AS grant_id,
                    UNNEST($4::timestamptz[]) AS used_at
            )
            UPDATE oauth_client_grants g
            SET
                last_used_at = GREATEST(COALESCE(g.last_used_at, input.used_at), input.used_at),
                updated_at = NOW()
            FROM input
            WHERE g.deployment_id = input.deployment_id
              AND g.oauth_client_id = input.oauth_client_id
              AND g.id = input.grant_id
            "#,
        )
        .bind(&deployment_ids)
        .bind(&client_ids)
        .bind(&grant_ids)
        .bind(&used_ats)
        .execute(&app_state.db_pool)
        .await?;

        Ok(synced)
    }
}

pub struct RevokeOAuthRefreshTokenFamily {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub root_refresh_token_id: i64,
}

impl Command for RevokeOAuthRefreshTokenFamily {
    type Output = u64;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let res = sqlx::query(
            r#"
            WITH RECURSIVE token_chain AS (
                SELECT id, replaced_by_token_id
                FROM oauth_refresh_tokens
                WHERE deployment_id = $1
                  AND oauth_client_id = $2
                  AND id = $3
                UNION ALL
                SELECT rt.id, rt.replaced_by_token_id
                FROM oauth_refresh_tokens rt
                INNER JOIN token_chain chain
                    ON rt.id = chain.replaced_by_token_id
                WHERE rt.deployment_id = $1
                  AND rt.oauth_client_id = $2
            )
            UPDATE oauth_refresh_tokens rt
            SET revoked_at = NOW()
            FROM token_chain
            WHERE rt.id = token_chain.id
              AND rt.revoked_at IS NULL
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.oauth_client_id)
        .bind(self.root_refresh_token_id)
        .execute(&app_state.db_pool)
        .await?;

        Ok(res.rows_affected())
    }
}

fn generate_token(prefix: &str, bytes_len: usize) -> String {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let mut bytes = vec![0u8; bytes_len];
    rand::rng().fill_bytes(&mut bytes);
    format!("{}_{}", prefix, URL_SAFE_NO_PAD.encode(bytes))
}

fn hash_value(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}
