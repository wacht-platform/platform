use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{AgentToolApprovalRule, AiKnowledgeBase, AiTool};

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct AgentModelOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::utils::serde::i64_as_string_option"
    )]
    pub profile_id: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentHookStep {
    pub tool_name: String,
    #[serde(default = "default_hook_args")]
    pub args: serde_json::Value,
}

fn default_hook_args() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct AgentHooksConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub execution_start: Vec<AgentHookStep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before_llm: Vec<AgentHookStep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after_llm: Vec<AgentHookStep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before_tool: Vec<AgentHookStep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after_tool: Vec<AgentHookStep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on_budget_exhausted: Vec<AgentHookStep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub execution_end: Vec<AgentHookStep>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AiAgent {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    pub description: Option<String>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    /// Agents this agent can spawn as sub-agents (empty = can only fork itself)
    #[serde(default, with = "crate::utils::serde::option_vec_i64_as_string")]
    pub sub_agents: Option<Vec<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strong_model: Option<AgentModelOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weak_model: Option<AgentModelOverride>,
    #[serde(default)]
    pub hooks: AgentHooksConfig,
    #[serde(default)]
    pub require_approval_mcp: bool,
    #[serde(default)]
    pub require_approval_virtual: bool,
    #[serde(default)]
    pub tool_approval_rules: Vec<AgentToolApprovalRule>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AiAgentWithDetails {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    pub description: Option<String>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub tools_count: i64,
    pub knowledge_bases_count: i64,
    /// Agents this agent can spawn as sub-agents
    #[serde(default, with = "crate::utils::serde::option_vec_i64_as_string")]
    pub sub_agents: Option<Vec<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strong_model: Option<AgentModelOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weak_model: Option<AgentModelOverride>,
    #[serde(default)]
    pub hooks: AgentHooksConfig,
    #[serde(default)]
    pub require_approval_mcp: bool,
    #[serde(default)]
    pub require_approval_virtual: bool,
    #[serde(default)]
    pub tool_approval_rules: Vec<AgentToolApprovalRule>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AiAgentWithFeatures {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    pub description: Option<String>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub tools: Vec<AiTool>,
    pub knowledge_bases: Vec<AiKnowledgeBase>,
    /// Agents this agent can spawn as sub-agents
    #[serde(default, with = "crate::utils::serde::option_vec_i64_as_string")]
    pub sub_agents: Option<Vec<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strong_model: Option<AgentModelOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weak_model: Option<AgentModelOverride>,
    #[serde(default)]
    pub hooks: AgentHooksConfig,
    #[serde(default)]
    pub require_approval_mcp: bool,
    #[serde(default)]
    pub require_approval_virtual: bool,
    #[serde(default)]
    pub tool_approval_rules: Vec<AgentToolApprovalRule>,
}
