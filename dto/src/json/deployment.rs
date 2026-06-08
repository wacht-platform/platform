use serde::{Deserialize, Serialize};

use models::{
    AgentHooksConfig, AgentLimits, AgentModelOverride, AgentToolApprovalRule, AiToolConfiguration,
    ApprovalAction, FlexibleI64,
};

#[derive(Deserialize)]
pub struct CreateAgentRequest {
    pub name: String,
    pub description: Option<String>,
    pub tool_ids: Option<Vec<FlexibleI64>>,
    pub knowledge_base_ids: Option<Vec<FlexibleI64>>,
    /// Agent IDs this agent can spawn as sub-agents
    pub sub_agents: Option<Vec<FlexibleI64>>,
    pub strong_model: Option<AgentModelOverride>,
    pub weak_model: Option<AgentModelOverride>,
    pub hooks: Option<AgentHooksConfig>,
    pub limits: Option<AgentLimits>,
    pub require_approval_mcp: Option<bool>,
    pub require_approval_virtual: Option<bool>,
    pub tool_approval_rules: Option<Vec<AgentToolApprovalRule>>,
}

#[derive(Deserialize)]
pub struct UpdateAgentRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub tool_ids: Option<Vec<FlexibleI64>>,
    pub knowledge_base_ids: Option<Vec<FlexibleI64>>,
    pub strong_model: Option<AgentModelOverride>,
    #[serde(default)]
    pub clear_strong_model: bool,
    pub weak_model: Option<AgentModelOverride>,
    #[serde(default)]
    pub clear_weak_model: bool,
    pub hooks: Option<AgentHooksConfig>,
    pub limits: Option<AgentLimits>,
    pub require_approval_mcp: Option<bool>,
    pub require_approval_virtual: Option<bool>,
    pub tool_approval_rules: Option<Vec<AgentToolApprovalRule>>,
}

#[derive(Deserialize)]
pub struct AttachToolRequest {
    #[serde(default)]
    pub approval_action: ApprovalAction,
}

#[derive(Deserialize)]
pub struct UpdateAgentToolApprovalActionRequest {
    pub approval_action: ApprovalAction,
}

#[derive(Deserialize)]
pub struct SetAgentRoleAgentRequest {
    /// Which role to set: "reviewer" or "conversation".
    pub role: String,
    /// Target agent id; null or omitted resets to the agent itself (the default).
    #[serde(default)]
    pub agent_id: Option<FlexibleI64>,
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
    pub agent_id: Option<FlexibleI64>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct CreateAgentThreadRequest {
    pub title: String,
    pub agent_id: Option<FlexibleI64>,
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

#[derive(Deserialize)]
pub struct UpdateActorProjectRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateAgentThreadRequest {
    pub title: Option<String>,
    pub agent_id: Option<FlexibleI64>,
    pub system_instructions: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateProjectTaskBoardItemRequest {
    pub title: String,
    pub description: Option<String>,
    pub status: Option<String>,
    pub schedule_kind: Option<String>,
    pub next_run_at: Option<chrono::DateTime<chrono::Utc>>,
    pub interval_seconds: Option<i64>,
    pub mounts: Option<Vec<models::project_task_schedule::ScheduleMount>>,
}

#[derive(Deserialize)]
pub struct UpdateProjectTaskBoardItemRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub schedule_kind: Option<String>,
    pub next_run_at: Option<chrono::DateTime<chrono::Utc>>,
    pub interval_seconds: Option<i64>,
    pub clear_schedule: Option<bool>,
    pub mounts: Option<Vec<models::project_task_schedule::ScheduleMount>>,
}

#[derive(Deserialize)]
pub struct SearchActorProjectsRequest {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub actor_id: i64,
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Deserialize)]
pub struct SearchActorProjectThreadsRequest {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub actor_id: i64,
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
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
