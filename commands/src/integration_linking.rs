use chrono::{Duration, Utc};
use common::error::AppError;
use models::ActiveAgentIntegration;
use serde::Serialize;

const BASE62_CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Encodes a number to Base62 string (guaranteed unique if input is unique)
fn base62_encode(mut num: u64) -> String {
    if num == 0 {
        return "0".to_string();
    }
    let mut result = Vec::new();
    while num > 0 {
        result.push(BASE62_CHARS[(num % 62) as usize]);
        num /= 62;
    }
    result.reverse();
    String::from_utf8(result).unwrap()
}

/// Command to create a new integration link code
pub struct CreateIntegrationLinkCodeCommand {
    id: Option<i64>,
    deployment_id: i64,
    context_group: String,
    agent_id: i64,
    integration_type: String,
}

impl CreateIntegrationLinkCodeCommand {
    pub fn new(
        deployment_id: i64,
        context_group: String,
        agent_id: i64,
        integration_type: String,
    ) -> Self {
        Self {
            id: None,
            deployment_id,
            context_group,
            agent_id,
            integration_type,
        }
    }

    pub fn with_id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<LinkCodeResponse, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = self
            .id
            .ok_or_else(|| AppError::Validation("id is required".to_string()))?;
        let code = base62_encode(id as u64);
        let expires_at = Utc::now() + Duration::minutes(10);

        sqlx::query!(
            r#"
            INSERT INTO integration_link_codes (id, deployment_id, context_group, agent_id, integration_type, code, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            id,
            self.deployment_id,
            self.context_group,
            self.agent_id,
            self.integration_type,
            code,
            expires_at,
        )
        .execute(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(LinkCodeResponse { code, expires_at })
    }
}

#[derive(Serialize)]
pub struct LinkCodeResponse {
    pub code: String,
    pub expires_at: chrono::DateTime<Utc>,
}

/// Command to validate a link code and create the connection
pub struct ValidateLinkCodeCommand {
    connection_id: Option<i64>,
    code: String,
    integration_id: i64,
    external_id: String,
    connection_metadata: serde_json::Value,
}

impl ValidateLinkCodeCommand {
    pub fn new(
        code: String,
        integration_id: i64,
        external_id: String,
        connection_metadata: serde_json::Value,
    ) -> Self {
        Self {
            connection_id: None,
            code,
            integration_id,
            external_id,
            connection_metadata,
        }
    }

    pub fn with_connection_id(mut self, connection_id: i64) -> Self {
        self.connection_id = Some(connection_id);
        self
    }

    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<ValidateLinkCodeResponse, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let connection_id = self
            .connection_id
            .ok_or_else(|| AppError::Validation("connection_id is required".to_string()))?;
        let row = sqlx::query!(
            r#"
            WITH link AS (
                UPDATE integration_link_codes
                SET used_at = NOW()
                WHERE code = $2 AND used_at IS NULL AND expires_at > NOW()
                RETURNING deployment_id, context_group
            ),
            upsert AS (
                INSERT INTO active_agent_integrations (
                    id, deployment_id, context_group, integration_id, external_id, connection_metadata
                )
                SELECT $1, link.deployment_id, link.context_group, $3, $4, $5
                FROM link
                ON CONFLICT (integration_id, external_id)
                DO UPDATE SET context_group = EXCLUDED.context_group, connection_metadata = EXCLUDED.connection_metadata, updated_at = NOW()
            )
            SELECT deployment_id, context_group
            FROM link
            "#,
            connection_id,
            self.code,
            self.integration_id,
            self.external_id,
            self.connection_metadata,
        )
        .fetch_optional(executor)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::BadRequest("Invalid or expired code".to_string()))?;

        Ok(ValidateLinkCodeResponse {
            context_group: row.context_group,
            deployment_id: row.deployment_id,
            connection_id,
        })
    }
}

#[derive(Serialize)]
pub struct ValidateLinkCodeResponse {
    pub context_group: String,
    pub deployment_id: i64,
    pub connection_id: i64,
}

/// Command to get user connection by external ID (read operation)
pub struct GetActiveIntegrationCommand {
    pub integration_id: i64,
    pub external_id: String,
}

impl GetActiveIntegrationCommand {
    pub fn new(integration_id: i64, external_id: String) -> Self {
        Self {
            integration_id,
            external_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Option<ActiveAgentIntegration>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query_as!(
            ActiveAgentIntegration,
            r#"
            SELECT id, deployment_id, context_group, integration_id, external_id, connection_metadata, created_at, updated_at
            FROM active_agent_integrations
            WHERE integration_id = $1 AND external_id = $2
            "#,
            self.integration_id,
            self.external_id,
        )
        .fetch_optional(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(result)
    }
}
