use crate::Command;
use chrono::Utc;
use common::error::AppError;
use common::state::AppState;
use models::scim_token::ScimToken;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize, Deserialize)]
pub struct GenerateScimTokenRequest {
    pub organization_id: i64,
    pub connection_id: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GenerateScimTokenResponse {
    pub token: ScimToken,
    pub plain_token: String,
}

pub struct GenerateScimTokenCommand {
    pub deployment_id: i64,
    pub request: GenerateScimTokenRequest,
}

impl GenerateScimTokenCommand {
    pub fn new(deployment_id: i64, request: GenerateScimTokenRequest) -> Self {
        Self {
            deployment_id,
            request,
        }
    }

    fn generate_token() -> (String, String, String) {
        let mut rng = rand::rng();
        let random_bytes: [u8; 32] = rng.random();
        let token_suffix = hex::encode(random_bytes);
        let plain_token = format!("scm_{}", token_suffix);
        let token_prefix = format!("scm_{}...", &token_suffix[..8]);
        
        let mut hasher = Sha256::new();
        hasher.update(plain_token.as_bytes());
        let token_hash = hex::encode(hasher.finalize());
        
        (plain_token, token_prefix, token_hash)
    }
}

impl Command for GenerateScimTokenCommand {
    type Output = GenerateScimTokenResponse;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Delete any existing token for this connection
        sqlx::query!(
            r#"
            DELETE FROM scim_tokens
            WHERE enterprise_connection_id = $1
            "#,
            self.request.connection_id
        )
        .execute(&app_state.db_pool)
        .await?;

        // Generate new token
        let (plain_token, token_prefix, token_hash) = Self::generate_token();
        let token_id = app_state.sf.next_id()? as i64;
        let now = Utc::now();

        let token = sqlx::query_as::<_, ScimToken>(
            r#"
            INSERT INTO scim_tokens (
                id,
                enterprise_connection_id,
                deployment_id,
                organization_id,
                token_hash,
                token_prefix,
                enabled,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING *
            "#,
        )
        .bind(token_id)
        .bind(self.request.connection_id)
        .bind(self.deployment_id)
        .bind(self.request.organization_id)
        .bind(&token_hash)
        .bind(&token_prefix)
        .bind(true)
        .bind(now)
        .bind(now)
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(GenerateScimTokenResponse { token, plain_token })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RevokeScimTokenRequest {
    pub organization_id: i64,
    pub connection_id: i64,
}

pub struct RevokeScimTokenCommand {
    pub deployment_id: i64,
    pub request: RevokeScimTokenRequest,
}

impl RevokeScimTokenCommand {
    pub fn new(deployment_id: i64, request: RevokeScimTokenRequest) -> Self {
        Self {
            deployment_id,
            request,
        }
    }
}

impl Command for RevokeScimTokenCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = sqlx::query!(
            r#"
            DELETE FROM scim_tokens
            WHERE enterprise_connection_id = $1 
              AND organization_id = $2 
              AND deployment_id = $3
            "#,
            self.request.connection_id,
            self.request.organization_id,
            self.deployment_id
        )
        .execute(&app_state.db_pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("SCIM token not found".to_string()));
        }

        Ok(())
    }
}
