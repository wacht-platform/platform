use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use super::{AiKnowledgeBase, AiTool, AiWorkflow, ExecutionStatus, NodeExecution};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub agent_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub execution_context_id: i64,
    pub tools: Vec<AiTool>,
    pub workflows: Vec<AiWorkflow>,
    pub knowledge_bases: Vec<AiKnowledgeBase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub result: Value,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWorkflowExecution {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub workflow_id: i64,
    pub execution_id: String,
    pub status: ExecutionStatus,
    pub current_node: Option<String>,
    pub execution_context: WorkflowExecutionContext,
    pub node_executions: HashMap<String, NodeExecution>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowExecutionContext {
    pub variables: HashMap<String, Value>,
    pub input_data: Value,
    pub output_data: Option<Value>,
    pub memory: HashMap<String, Value>,
    pub tool_results: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeExecutionResult {
    pub status: ExecutionStatus,
    pub output_data: Option<Value>,
    pub error_message: Option<String>,
    pub next_nodes: Vec<String>,
    pub execution_time_ms: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextRecord {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub agent_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub execution_context_id: i64,
    pub key: String,
    pub data: Value,
    pub metadata: Option<Value>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub updated_at: DateTime<Utc>,
}
