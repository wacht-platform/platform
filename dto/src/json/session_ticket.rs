use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionIdentifierDto {
    Static,
    Signin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionTicketRequest {
    pub ticket_type: String,
    pub user_id: Option<String>,
    pub actor_id: Option<String>,
    pub agent_ids: Option<Vec<String>>,
    pub agent_session_identifier: Option<AgentSessionIdentifierDto>,
    pub webhook_app_slug: Option<String>,
    pub api_auth_app_slug: Option<String>,
    pub expires_in: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTicketTypeDto {
    pub impersonation: String,
    pub agent_access: String,
}
