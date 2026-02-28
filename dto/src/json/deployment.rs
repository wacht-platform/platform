use serde::{Deserialize, Serialize};

use models::{AiToolConfiguration, SpawnConfig};

#[derive(Deserialize)]
pub struct CreateAgentRequest {
    pub name: String,
    pub description: Option<String>,
    pub configuration: Option<serde_json::Value>,
    pub tool_ids: Option<Vec<i64>>,
    pub knowledge_base_ids: Option<Vec<i64>>,
    /// Agent IDs this agent can spawn as sub-agents
    pub sub_agents: Option<Vec<i64>>,
    /// Spawn configuration
    pub spawn_config: Option<SpawnConfig>,
}

#[derive(Deserialize)]
pub struct UpdateAgentRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub configuration: Option<serde_json::Value>,
    pub tool_ids: Option<Vec<i64>>,
    pub knowledge_base_ids: Option<Vec<i64>>,
    /// Agent IDs this agent can spawn as sub-agents
    pub sub_agents: Option<Vec<i64>>,
    /// Spawn configuration
    pub spawn_config: Option<SpawnConfig>,
}

#[derive(Deserialize)]
pub struct CreateToolRequest {
    pub name: String,
    pub description: Option<String>,
    pub tool_type: String,
    pub configuration: AiToolConfiguration,
}

#[derive(Deserialize)]
pub struct UpdateToolRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub tool_type: Option<String>,
    pub configuration: Option<AiToolConfiguration>,
}

// AI Execution Context models
#[derive(Deserialize)]
pub struct CreateExecutionContextRequest {
    pub title: Option<String>,
    pub system_instructions: Option<String>,
    pub context_group: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateExecutionContextRequest {
    pub title: Option<String>,
    pub system_instructions: Option<String>,
    pub context_group: Option<String>,
    pub status: Option<String>,
}

#[derive(Deserialize)]
pub struct NewMessageRequest {
    pub message: String,
    pub files: Option<Vec<crate::json::agent_executor::FileData>>,
}

#[derive(Deserialize)]
pub struct UserInputResponseRequest {
    pub message: String,
}

#[derive(Deserialize)]
pub struct PlatformFunctionResultRequest {
    pub execution_id: String,
    pub result: serde_json::Value,
}

#[derive(Deserialize)]
pub struct CancelRequest {}

#[derive(Deserialize)]
pub struct ExecuteAgentRequestType {
    pub new_message: Option<NewMessageRequest>,
    pub user_input_response: Option<UserInputResponseRequest>,
    pub platform_function_result: Option<PlatformFunctionResultRequest>,
    pub cancel: Option<CancelRequest>,
}

#[derive(Deserialize)]
pub struct ExecuteAgentRequest {
    pub agent_name: Option<String>,
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
