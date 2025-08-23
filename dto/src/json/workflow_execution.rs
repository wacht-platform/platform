use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// Workflow Execution Results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowExecutionResult {
    pub workflow_id: i64,
    pub workflow_name: String,
    pub execution_status: String, // "pending", "completed", "failed"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTaskExecution {
    #[serde(rename = "type")]
    pub execution_type: String, // "workflow"
    pub workflow_id: i64,
    pub result: WorkflowExecutionResult,
}

// Node Execution Results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerNodeResult {
    #[serde(rename = "type")]
    pub node_type: String, // "trigger"
    pub triggered: bool,
    pub description: String,
    pub trigger_condition: String,
    pub evaluation: TriggerEvaluation,
    pub context: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerEvaluation {
    pub reasoning: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMNodeResult {
    #[serde(rename = "type")]
    pub node_type: String, // "llm_response"
    pub format: String, // "text" or "json"
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchNodeResult {
    #[serde(rename = "type")]
    pub node_type: String, // "switch"
    pub matched_case: Value, // Can be number or "default"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub case_label: Option<String>,
    pub switch_value: Value,
    pub reasoning: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInputNodeResult {
    pub status: String, // "pending"
    #[serde(rename = "type")]
    pub node_type: String, // "user_input"
    pub input_type: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
}

// Workflow State Components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowContextSummary {
    pub inputs: Value,
    pub total_context_items: i32,
    pub has_conversation_history: bool,
    pub has_memory_context: bool,
    #[serde(flatten)]
    pub node_outputs: HashMap<String, Value>, // Keys ending with "_output"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInputData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_context: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_context: Option<Vec<Value>>,
    pub inputs: HashMap<String, Value>,
    pub total_context_items: usize,
}