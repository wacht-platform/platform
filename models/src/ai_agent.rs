use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{AiKnowledgeBase, AiTool};

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct AgentModelOverride {
    pub provider: String,
    pub model: String,
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
    pub configuration: serde_json::Value,
    /// Agents this agent can spawn as sub-agents (empty = can only fork itself)
    pub sub_agents: Option<Vec<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strong_model: Option<AgentModelOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weak_model: Option<AgentModelOverride>,
    #[serde(default)]
    pub hooks: AgentHooksConfig,
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
    pub configuration: serde_json::Value,
    pub tools_count: i64,
    pub knowledge_bases_count: i64,
    /// Agents this agent can spawn as sub-agents
    pub sub_agents: Option<Vec<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strong_model: Option<AgentModelOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weak_model: Option<AgentModelOverride>,
    #[serde(default)]
    pub hooks: AgentHooksConfig,
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
    pub configuration: serde_json::Value,
    pub tools: Vec<AiTool>,
    pub knowledge_bases: Vec<AiKnowledgeBase>,
    /// Agents this agent can spawn as sub-agents
    pub sub_agents: Option<Vec<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strong_model: Option<AgentModelOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weak_model: Option<AgentModelOverride>,
    #[serde(default)]
    pub hooks: AgentHooksConfig,
}
