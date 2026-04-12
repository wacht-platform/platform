use serde::{Deserialize, Serialize};
use serde_json::Value;

// Tool Execution Results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToolResult {
    pub success: bool,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub tool: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformEventResult {
    pub success: bool,
    pub tool: String,
    pub event_label: String,
    pub event_data: Value,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolKnowledgeBaseSearchResult {
    pub content: String,
    pub knowledge_base_id: String,
    pub similarity_score: f64,
    pub chunk_index: i32,
    pub document_id: String,
    pub document_title: Option<String>,
    pub document_description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeBaseToolResult {
    pub success: bool,
    pub tool: String,
    pub query: String,
    pub knowledge_base_ids: Vec<i64>,
    pub results: Vec<ToolKnowledgeBaseSearchResult>,
    pub total_results: usize,
    pub search_settings: Value,
}

// Task Execution Results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecutionResult {
    pub approach: String,
    pub actions: Vec<Value>,
    pub expected_result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_result: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub total_tasks: usize,
    pub tasks: Vec<Value>, // ExecutableTask structs
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedTaskResult {
    pub success: bool,
    pub blocked: bool,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing_dependencies: Option<Vec<String>>,
}
