use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{AiKnowledgeBase, AiTool};

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
}
