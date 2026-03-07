use chrono::Utc;
use common::error::AppError;
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
    token_id: Option<i64>,
    deployment_id: i64,
    request: GenerateScimTokenRequest,
}

#[derive(Default)]
pub struct GenerateScimTokenCommandBuilder {
    token_id: Option<i64>,
    deployment_id: Option<i64>,
    request: Option<GenerateScimTokenRequest>,
}

impl GenerateScimTokenCommand {
    pub fn builder() -> GenerateScimTokenCommandBuilder {
        GenerateScimTokenCommandBuilder::default()
    }

    pub fn new(deployment_id: i64, request: GenerateScimTokenRequest) -> Self {
        Self {
            token_id: None,
            deployment_id,
            request,
        }
    }

    pub fn with_token_id(mut self, token_id: i64) -> Self {
        self.token_id = Some(token_id);
        self
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

impl GenerateScimTokenCommand {
    pub async fn execute_with_db(
        self,
        acquirer: impl for<'a> sqlx::Acquire<'a, Database = sqlx::Postgres>,
    ) -> Result<GenerateScimTokenResponse, AppError> {
        let mut conn = acquirer.acquire().await?;
        let token_id = self
            .token_id
            .ok_or_else(|| AppError::Validation("token_id is required".to_string()))?;
        // Delete any existing token for this connection
        sqlx::query!(
            r#"
            DELETE FROM scim_tokens
            WHERE enterprise_connection_id = $1
            "#,
            self.request.connection_id
        )
        .execute(&mut *conn)
        .await?;

        // Generate new token
        let (plain_token, token_prefix, token_hash) = Self::generate_token();
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
        .fetch_one(&mut *conn)
        .await?;

        Ok(GenerateScimTokenResponse { token, plain_token })
    }
}

impl GenerateScimTokenCommandBuilder {
    pub fn token_id(mut self, token_id: i64) -> Self {
        self.token_id = Some(token_id);
        self
    }

    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn request(mut self, request: GenerateScimTokenRequest) -> Self {
        self.request = Some(request);
        self
    }

    pub fn build(self) -> Result<GenerateScimTokenCommand, AppError> {
        Ok(GenerateScimTokenCommand {
            token_id: self.token_id,
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
            request: self
                .request
                .ok_or_else(|| AppError::Validation("request is required".to_string()))?,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RevokeScimTokenRequest {
    pub organization_id: i64,
    pub connection_id: i64,
}

pub struct RevokeScimTokenCommand {
    deployment_id: i64,
    request: RevokeScimTokenRequest,
}

#[derive(Default)]
pub struct RevokeScimTokenCommandBuilder {
    deployment_id: Option<i64>,
    request: Option<RevokeScimTokenRequest>,
}

impl RevokeScimTokenCommand {
    pub fn builder() -> RevokeScimTokenCommandBuilder {
        RevokeScimTokenCommandBuilder::default()
    }

    pub fn new(deployment_id: i64, request: RevokeScimTokenRequest) -> Self {
        Self {
            deployment_id,
            request,
        }
    }
}

impl RevokeScimTokenCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
        .execute(executor)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("SCIM token not found".to_string()));
        }

        Ok(())
    }
}

impl RevokeScimTokenCommandBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn request(mut self, request: RevokeScimTokenRequest) -> Self {
        self.request = Some(request);
        self
    }

    pub fn build(self) -> Result<RevokeScimTokenCommand, AppError> {
        Ok(RevokeScimTokenCommand {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
            request: self
                .request
                .ok_or_else(|| AppError::Validation("request is required".to_string()))?,
        })
    }
}
