use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionTicketRequest {
    pub ticket_type: String,
    pub user_id: Option<String>,
    pub agent_ids: Option<Vec<String>>,
    pub context_group: Option<String>,
    pub expires_in: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTicketTypeDto {
    pub impersonation: String,
    pub agent_access: String,
}
