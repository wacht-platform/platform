use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// Template Context for LLM Calls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDecisionContext {
    pub current_datetime_utc: String,
    pub conversation_history: Vec<Value>,
    pub user_request: String,
    #[serde(default)]
    pub input_safety_signals: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_objective: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_insights: Option<Value>,
    pub task_results: HashMap<String, Value>,
    #[serde(default)]
    pub loaded_memories: Vec<Value>,
    pub available_tools: Vec<Value>,
    pub available_knowledge_bases: Vec<Value>,
    #[serde(default)]
    pub available_sub_agents: Vec<SubAgentPromptInfo>,
    #[serde(default)]
    pub supervisor_mode_active: bool,
    #[serde(default)]
    pub supervisor_task_board: Vec<Value>,
    #[serde(default)]
    pub is_child_context: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_context_id: Option<i64>,
    pub iteration_info: IterationInfo,
    #[serde(default)]
    pub teams_enabled: bool,
    #[serde(default)]
    pub clickup_enabled: bool,
    #[serde(default)]
    pub mcp_enabled: bool,
    #[serde(default)]
    pub deep_think_mode_active: bool,
    #[serde(default)]
    pub deep_think_used: usize,
    #[serde(default)]
    pub deep_think_remaining: usize,
    #[serde(default = "default_deep_think_max_uses")]
    pub deep_think_max_uses: usize,
    pub context_id: i64,
    pub context_title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub teams_context: Option<TeamsContextInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentPromptInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Teams-specific context information for agent awareness
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamsContextInfo {
    /// Type of conversation: "channel", "groupChat", or "personal"
    pub conversation_type: String,
    /// Name of the team/channel or "Personal"
    pub channel_name: String,
    /// Team ID (for channel meetings)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationInfo {
    pub current_iteration: usize,
    pub max_iterations: usize,
}

fn default_deep_think_max_uses() -> usize {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationContext {
    pub conversation_history: Vec<Value>,
    pub user_request: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_objective: Option<Value>,
    pub task_results: HashMap<String, Value>,
    pub available_tools: Vec<Value>,
    pub available_knowledge_bases: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInputRequestContext {
    pub conversation_history: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_objective: Option<Value>,
    pub working_memory: HashMap<String, Value>,
    pub available_tools: Vec<Value>,
    pub available_knowledge_bases: Vec<Value>,
}

// LLM Generation Config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMGenerationConfig {
    pub contents: Vec<LLMContent>,
    #[serde(rename = "generationConfig")]
    pub generation_config: GenerationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMContent {
    pub parts: Vec<LLMPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMPart {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationConfig {
    pub temperature: f32,
    #[serde(rename = "topK")]
    pub top_k: i32,
    #[serde(rename = "topP")]
    pub top_p: f32,
    #[serde(rename = "maxOutputTokens")]
    pub max_output_tokens: i32,
}
