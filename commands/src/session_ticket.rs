use common::error::AppError;
use common::state::AppState;
use chrono::Utc;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionTicketType {
    #[serde(rename = "impersonation")]
    Impersonation,
    #[serde(rename = "agent_access")]
    AgentAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTicketPayload {
    pub ticket_type: SessionTicketType,
    pub deployment_id: String,
    pub user_id: Option<String>,
    pub agent_ids: Option<Vec<String>>,
    pub context_group: Option<String>,
    pub expires_at: i64,
}

pub struct GenerateSessionTicketCommand {
    pub deployment_id: i64,
    pub ticket_type: SessionTicketType,
    pub user_id: Option<String>,
    pub agent_ids: Option<Vec<String>>,
    pub context_group: Option<String>,
    pub expires_in: Option<u64>,
}

impl GenerateSessionTicketCommand {
    pub fn new(
        deployment_id: i64,
        ticket_type: SessionTicketType,
    ) -> Self {
        Self {
            deployment_id,
            ticket_type,
            user_id: None,
            agent_ids: None,
            context_group: None,
            expires_in: None,
        }
    }

    pub fn with_user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn with_agent_ids(mut self, agent_ids: Vec<String>) -> Self {
        self.agent_ids = Some(agent_ids);
        self
    }

    pub fn with_context_group(mut self, context_group: String) -> Self {
        self.context_group = Some(context_group);
        self
    }

    pub fn with_expires_in(mut self, expires_in: u64) -> Self {
        self.expires_in = Some(expires_in);
        self
    }
}

#[derive(Debug, Serialize)]
pub struct GenerateSessionTicketResponse {
    pub ticket: String,
    pub expires_at: i64,
}

impl crate::Command for GenerateSessionTicketCommand {
    type Output = GenerateSessionTicketResponse;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Validate inputs based on ticket type
        match self.ticket_type {
            SessionTicketType::Impersonation => {
                if self.user_id.is_none() {
                    return Err(AppError::BadRequest(
                        "user_id is required for impersonation tickets".to_string(),
                    ));
                }
            }
            SessionTicketType::AgentAccess => {
                if self.agent_ids.is_none() || self.agent_ids.as_ref().unwrap().is_empty() {
                    return Err(AppError::BadRequest(
                        "agent_ids is required for agent_access tickets".to_string(),
                    ));
                }
            }
        }

        // Generate ticket ID using Snowflake
        let ticket_id = app_state
            .sf
            .next_id()
            .map_err(|e| AppError::Internal(format!("Failed to generate ticket ID: {}", e)))?;
        let ticket = ticket_id.to_string();

        // Calculate expiration
        let ttl_seconds = self.expires_in.unwrap_or(43200); // Default 12 hours
        let expires_at = Utc::now().timestamp() + ttl_seconds as i64;

        // Create payload
        let payload = SessionTicketPayload {
            ticket_type: self.ticket_type.clone(),
            deployment_id: self.deployment_id.to_string(),
            user_id: self.user_id,
            agent_ids: self.agent_ids,
            context_group: self.context_group,
            expires_at,
        };

        // Serialize and store in Redis
        let payload_json =
            serde_json::to_string(&payload).map_err(|e| {
                AppError::Internal(format!("Failed to serialize ticket: {}", e))
            })?;

        let redis_key = format!("session:ticket:{}", ticket);

        let mut conn = app_state
            .redis_client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| {
                AppError::Internal(format!("Failed to connect to Redis: {}", e))
            })?;

        conn.set_ex::<_, _, ()>(&redis_key, &payload_json, ttl_seconds)
            .await
            .map_err(|e| {
                AppError::Internal(format!("Failed to store ticket in Redis: {}", e))
            })?;

        Ok(GenerateSessionTicketResponse { ticket, expires_at })
    }
}
