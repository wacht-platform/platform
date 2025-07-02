use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::models::{AiKnowledgeBase, AiTool, AiWorkflow};

#[derive(Debug, Serialize, Deserialize, Clone)]
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
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    pub workflows_count: i64,
    pub knowledge_bases_count: i64,
}

pub struct AiAgentWithFeatures {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    pub deployment_id: i64,
    pub configuration: serde_json::Value,
    pub tools: Vec<AiTool>,
    pub workflows: Vec<AiWorkflow>,
    pub knowledge_bases: Vec<AiKnowledgeBase>,
}
