use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::models::SchemaField;

#[derive(Serialize, Deserialize, Clone)]
pub struct AiWorkflow {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub configuration: WorkflowConfiguration,
    pub workflow_definition: WorkflowDefinition,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AiWorkflowWithDetails {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub configuration: WorkflowConfiguration,
    pub workflow_definition: WorkflowDefinition,
    pub agents_count: i64,
    pub last_execution_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct WorkflowConfiguration {
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
    #[serde(default)]
    pub max_retries: Option<u32>,
    #[serde(default)]
    pub retry_delay_seconds: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct WorkflowDefinition {
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
    pub version: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct WorkflowNode {
    pub id: String,
    pub node_type: WorkflowNodeType,
    pub position: NodePosition,
    pub data: WorkflowNodeData,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NodePosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WorkflowNodeType {
    Trigger(TriggerNodeConfig),
    ErrorHandler(ErrorHandlerNodeConfig),
    LLMCall(LLMCallNodeConfig),
    Switch(SwitchNodeConfig),
    ToolCall(ToolCallNodeConfig),
    UserInput(UserInputNodeConfig),
}

impl WorkflowNodeType {
    pub fn type_name(&self) -> &'static str {
        match self {
            WorkflowNodeType::Trigger(_) => "Trigger",
            WorkflowNodeType::ErrorHandler(_) => "ErrorHandler",
            WorkflowNodeType::LLMCall(_) => "LLMCall",
            WorkflowNodeType::Switch(_) => "Switch",
            WorkflowNodeType::ToolCall(_) => "ToolCall",
            WorkflowNodeType::UserInput(_) => "UserInput",
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct WorkflowNodeData {
    pub label: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub config: serde_json::Value,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct WorkflowEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub source_handle: Option<String>,
    pub target_handle: Option<String>,
}


// Node-specific configurations
#[derive(Serialize, Deserialize, Clone)]
pub struct TriggerNodeConfig {
    #[serde(default)]
    pub description: Option<String>, // Natural language description of what data/conditions are needed for this workflow to run
    pub condition: String, // Natural language condition that describes when this trigger should activate
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ErrorHandlerNodeConfig {
    #[serde(default)]
    pub description: Option<String>,
    pub enable_retry: bool,
    pub max_retries: u32,
    pub retry_delay_seconds: u32,
    pub log_errors: bool,
    pub custom_error_message: Option<String>,
    pub contained_nodes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LLMCallNodeConfig {
    #[serde(default)]
    pub description: Option<String>,
    pub prompt_template: String,
    pub response_format: ResponseFormat,
    pub json_schema: Vec<SchemaField>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ResponseFormat {
    Text,
    Json,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SwitchNodeConfig {
    #[serde(default)]
    pub description: Option<String>,
    pub switch_condition: String,
    pub cases: Vec<SwitchCase>,
    pub default_case: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ComparisonType {
    Equals,
    Contains,
    StartsWith,
    EndsWith,
    Regex,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SwitchCase {
    pub case_condition: String,
    pub case_label: Option<String>,
}


#[derive(Serialize, Deserialize, Clone)]
pub struct ToolCallNodeConfig {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub tool_id: i64,
    pub input_parameters: HashMap<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UserInputNodeConfig {
    #[serde(default)]
    pub description: Option<String>,
    pub prompt: String, // The message to show to the user
    pub input_type: UserInputType,
    pub default_value: Option<String>,
    pub placeholder: Option<String>,
    pub options: Option<Vec<String>>, // For select/multi-select types
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum UserInputType {
    Text,
    Number,
    Select,
    MultiSelect,
    Boolean,
    Date,
}


// Workflow execution models
#[derive(Serialize, Deserialize, Clone)]
pub struct WorkflowExecution {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub workflow_id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub status: ExecutionStatus,
    pub trigger_data: Option<serde_json::Value>,
    pub execution_context: ExecutionContext,
    pub output_data: Option<serde_json::Value>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Timeout,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ExecutionContext {
    pub variables: HashMap<String, serde_json::Value>,
    pub node_executions: Vec<NodeExecution>,
    pub current_node: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NodeExecution {
    pub node_id: String,
    pub status: ExecutionStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub input_data: Option<serde_json::Value>,
    pub output_data: Option<serde_json::Value>,
    pub error_message: Option<String>,
    pub retry_count: u32,
}

// Default implementations
impl Default for WorkflowConfiguration {
    fn default() -> Self {
        Self {
            timeout_seconds: Some(300), // 5 minutes
            max_retries: Some(3),
            retry_delay_seconds: Some(5),
        }
    }
}

impl Default for WorkflowDefinition {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            version: "1.0.0".to_string(),
        }
    }
}

impl From<String> for ExecutionStatus {
    fn from(status: String) -> Self {
        match status.to_lowercase().as_str() {
            "pending" => ExecutionStatus::Pending,
            "running" => ExecutionStatus::Running,
            "completed" => ExecutionStatus::Completed,
            "failed" => ExecutionStatus::Failed,
            "cancelled" => ExecutionStatus::Cancelled,
            "timeout" => ExecutionStatus::Timeout,
            _ => ExecutionStatus::Pending,
        }
    }
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self {
            variables: HashMap::new(),
            node_executions: Vec::new(),
            current_node: None,
        }
    }
}
