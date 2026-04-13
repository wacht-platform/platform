use serde::{Deserialize, Serialize};

use models::AiToolConfiguration;

#[derive(Deserialize)]
pub struct CreateAgentRequest {
    pub name: String,
    pub description: Option<String>,
    pub configuration: Option<serde_json::Value>,
    pub tool_ids: Option<Vec<i64>>,
    pub knowledge_base_ids: Option<Vec<i64>>,
    /// Agent IDs this agent can spawn as sub-agents
    pub sub_agents: Option<Vec<i64>>,
}

#[derive(Deserialize)]
pub struct UpdateAgentRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub configuration: Option<serde_json::Value>,
    pub tool_ids: Option<Vec<i64>>,
    pub knowledge_base_ids: Option<Vec<i64>>,
    pub sub_agents: Option<Vec<i64>>,
}

#[derive(Deserialize)]
pub struct CreateToolRequest {
    pub name: String,
    pub description: Option<String>,
    pub tool_type: String,
    #[serde(default)]
    pub requires_user_approval: bool,
    pub configuration: AiToolConfiguration,
}

#[derive(Deserialize)]
pub struct UpdateToolRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub tool_type: Option<String>,
    pub requires_user_approval: Option<bool>,
    pub configuration: Option<AiToolConfiguration>,
}

#[derive(Deserialize)]
pub struct CreateActorRequest {
    pub subject_type: String,
    pub external_key: String,
    pub display_name: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct CreateActorProjectRequest {
    pub name: String,
    pub description: Option<String>,
    pub status: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct CreateAgentThreadRequest {
    pub title: String,
    pub system_instructions: Option<String>,
    pub thread_purpose: Option<String>,
    pub responsibility: Option<String>,
    pub reusable: Option<bool>,
    pub accepts_assignments: Option<bool>,
    pub capability_tags: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct NewMessageRequest {
    pub message: String,
    pub files: Option<Vec<crate::json::agent_executor::FileData>>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ToolApprovalSelection {
    pub tool_name: String,
    pub mode: models::ToolApprovalMode,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ApprovalResponseRequest {
    pub request_message_id: String,
    #[serde(default)]
    pub approvals: Vec<ToolApprovalSelection>,
}

#[derive(Deserialize)]
pub struct CancelRequest {}

#[derive(Deserialize)]
pub struct ExecuteAgentRequestType {
    pub new_message: Option<NewMessageRequest>,
    pub approval_response: Option<ApprovalResponseRequest>,
    pub cancel: Option<CancelRequest>,
}

#[derive(Deserialize)]
pub struct ExecuteAgentRequest {
    pub agent_id: Option<String>,
    pub execution_type: ExecuteAgentRequestType,
}

#[derive(Serialize)]
pub struct ExecuteAgentResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
}

#[derive(Serialize)]
pub struct UploadResult {
    pub url: String,
}
