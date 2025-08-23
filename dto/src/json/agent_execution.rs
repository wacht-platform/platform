use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use chrono::{DateTime, Utc};

// Memory and Context Structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingMemory {
    pub user_request: String,
    pub conversation_count: usize,
    pub pending_tasks_count: usize,
    pub completed_tasks_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub successful_task_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationHistoryEntry {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMetadata {
    pub source: String,
    pub relevance_score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRecord {
    pub source: String,
    pub relevance_score: f64,
    pub content: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorContext {
    pub error_message: String,
    pub tool_name: String,
    pub timestamp: DateTime<Utc>,
}

// Action and Tool Execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub success: bool,
    pub action_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub tool_name: String,
    pub parameters: HashMap<String, Value>,
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCallResult {
    pub workflow_name: String,
    pub inputs: HashMap<String, Value>,
    pub result: Value,
}

// Parameter Generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToolParameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_params: Option<HashMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeBaseParameters {
    pub query: String,
}

// Task Execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecutionSuccess {
    pub success: bool,
    pub task_id: String,
    pub output: Value,
}

// User Input State
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInputOutputState {
    pub value: String,
    #[serde(rename = "type")]
    pub output_type: String, // "user_input"
}

// Platform Function Result Storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformFunctionResultStorage {
    pub execution_id: String,
    pub result: Value,
}