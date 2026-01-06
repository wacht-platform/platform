use chrono::{Duration, Utc};
use common::error::AppError;
use common::state::AppState;
use models::{ActiveAgentIntegration, IntegrationLinkCode};
use serde::Serialize;

use crate::Command;

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
            deployment_id,
            context_group,
            agent_id,
            integration_type,
        }
    }
}

#[derive(Serialize)]
pub struct LinkCodeResponse {
    pub code: String,
    pub expires_at: chrono::DateTime<Utc>,
}

impl Command for CreateIntegrationLinkCodeCommand {
    type Output = LinkCodeResponse;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let id = app_state.sf.next_id().map_err(|e| AppError::Internal(format!("Failed to generate ID: {}", e)))? as i64;
        // Use Base62-encoded Snowflake ID for guaranteed uniqueness
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
        .execute(&app_state.db_pool)
        .await
        .map_err(AppError::Database)?;

        Ok(LinkCodeResponse { code, expires_at })
    }
}

/// Command to validate a link code and create the connection
pub struct ValidateLinkCodeCommand {
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
            code,
            integration_id,
            external_id,
            connection_metadata,
        }
    }
}

#[derive(Serialize)]
pub struct ValidateLinkCodeResponse {
    pub context_group: String,
    pub deployment_id: i64,
    pub connection_id: i64,
}

impl Command for ValidateLinkCodeCommand {
    type Output = ValidateLinkCodeResponse;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Find the code
        let link_code = sqlx::query_as!(
            IntegrationLinkCode,
            r#"
            SELECT id, deployment_id, context_group, agent_id, integration_type, code, expires_at, used_at, created_at
            FROM integration_link_codes
            WHERE code = $1 AND used_at IS NULL AND expires_at > NOW()
            "#,
            self.code,
        )
        .fetch_optional(&app_state.db_pool)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::BadRequest("Invalid or expired code".to_string()))?;

        // Mark code as used
        sqlx::query!(
            "UPDATE integration_link_codes SET used_at = NOW() WHERE id = $1",
            link_code.id,
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(AppError::Database)?;

        // Use context_group directly from link code
        let context_group = link_code.context_group.clone();

        // Create the active integration connection
        let connection_id = app_state.sf.next_id().map_err(|e| AppError::Internal(format!("Failed to generate ID: {}", e)))? as i64;
        sqlx::query!(
            r#"
            INSERT INTO active_agent_integrations (id, deployment_id, context_group, integration_id, external_id, connection_metadata)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (integration_id, external_id) 
            DO UPDATE SET context_group = $3, connection_metadata = $6, updated_at = NOW()
            "#,
            connection_id,
            link_code.deployment_id,
            context_group,
            self.integration_id,
            self.external_id,
            self.connection_metadata,
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(AppError::Database)?;

        Ok(ValidateLinkCodeResponse {
            context_group,
            deployment_id: link_code.deployment_id,
            connection_id,
        })
    }
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
}

impl Command for GetActiveIntegrationCommand {
    type Output = Option<ActiveAgentIntegration>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .fetch_optional(&app_state.db_pool)
        .await
        .map_err(AppError::Database)?;

        Ok(result)
    }
}
