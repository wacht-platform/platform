use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct GenerateUserAgentContextTokenRequest {
    pub agent_name: String,
    pub audience: Option<String>,
    pub validity_hours: Option<u32>,
}
