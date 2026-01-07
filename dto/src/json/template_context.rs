use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// Template Context for LLM Calls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDecisionContext {
    pub conversation_history: Vec<Value>,
    pub user_request: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_objective: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_insights: Option<Value>,
    pub task_results: HashMap<String, Value>,
    pub available_tools: Vec<Value>,
    pub available_workflows: Vec<Value>,
    pub available_knowledge_bases: Vec<Value>,
    pub iteration_info: IterationInfo,
    #[serde(default)]
    pub teams_enabled: bool,
    // Cross-context awareness
    pub context_id: i64,
    pub context_title: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub actionables: Vec<Actionable>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actionable {
    pub id: String,
    #[serde(rename = "type")]
    pub actionable_type: String,
    pub description: String,
    /// Stored as string to preserve precision for large IDs
    pub target_context_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationInfo {
    pub current_iteration: usize,
    pub max_iterations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationContext {
    pub conversation_history: Vec<Value>,
    pub user_request: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_objective: Option<Value>,
    pub task_results: HashMap<String, Value>,
    pub available_tools: Vec<Value>,
    pub available_workflows: Vec<Value>,
    pub available_knowledge_bases: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInputRequestContext {
    pub conversation_history: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_objective: Option<Value>,
    pub working_memory: HashMap<String, Value>,
    pub available_tools: Vec<Value>,
    pub available_workflows: Vec<Value>,
    pub available_knowledge_bases: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerEvaluationContext {
    pub trigger_condition: String,
    pub trigger_description: String,
    pub workflow_state: WorkflowStateSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStateSummary {
    pub inputs: Value,
    pub total_context_items: i32,
    pub has_conversation_history: bool,
    pub has_memory_context: bool,
    #[serde(flatten)]
    pub outputs: HashMap<String, Value>, // node outputs
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchCaseContext {
    pub switch_value: Value,
    pub cases: Vec<CaseDescription>,
    pub has_default: bool,
    pub workflow_state: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseDescription {
    pub index: usize,
    pub label: String,
    pub condition: String,
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
