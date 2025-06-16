use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AiWorkflow {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    pub description: Option<String>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub configuration: WorkflowConfiguration,
    pub workflow_definition: WorkflowDefinition,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AiWorkflowWithDetails {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    pub description: Option<String>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub configuration: WorkflowConfiguration,
    pub workflow_definition: WorkflowDefinition,
    pub agents_count: i64,
    pub last_execution_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowConfiguration {
    pub timeout_seconds: Option<u32>,
    pub max_retries: Option<u32>,
    pub retry_delay_seconds: Option<u32>,
    pub enable_logging: bool,
    pub enable_metrics: bool,
    pub variables: HashMap<String, WorkflowVariable>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowVariable {
    pub name: String,
    pub value_type: VariableType,
    pub default_value: Option<String>,
    pub description: Option<String>,
    pub required: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum VariableType {
    String,
    Number,
    Boolean,
    Object,
    Array,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowDefinition {
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowNode {
    pub id: String,
    pub node_type: WorkflowNodeType,
    pub position: NodePosition,
    pub data: WorkflowNodeData,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NodePosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WorkflowNodeType {
    Trigger(TriggerNodeConfig),
    Condition(ConditionNodeConfig),
    ErrorHandler(ErrorHandlerNodeConfig),
    LLMCall(LLMCallNodeConfig),
    Switch(SwitchNodeConfig),
    ToolCall(ToolCallNodeConfig),
    StoreContext(StoreContextNodeConfig),
    FetchContext(FetchContextNodeConfig),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowNodeData {
    pub label: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub config: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub source_handle: Option<String>,
    pub target_handle: Option<String>,
    pub condition: Option<EdgeCondition>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EdgeCondition {
    pub expression: String,
    pub condition_type: ConditionType,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ConditionType {
    Always,
    OnSuccess,
    OnError,
    OnCondition,
}

// Node-specific configurations
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TriggerNodeConfig {
    pub condition: String, // Text condition for automated trigger
}



#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ErrorHandlerNodeConfig {
    pub enable_retry: bool,
    pub max_retries: u32,
    pub retry_delay_seconds: u32,
    pub log_errors: bool,
    pub custom_error_message: Option<String>,
    pub contained_nodes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LLMCallNodeConfig {
    pub prompt_template: String,
    pub response_format: ResponseFormat,
    pub json_schema: Vec<SchemaField>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ResponseFormat {
    Text,
    Json,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SchemaField {
    pub name: String,
    pub field_type: String,
    pub required: bool,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SwitchNodeConfig {
    pub switch_condition: String,
    pub cases: Vec<SwitchCase>,
    pub default_case: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ComparisonType {
    Equals,
    Contains,
    StartsWith,
    EndsWith,
    Regex,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SwitchCase {
    pub case_condition: String,
    pub case_label: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConditionNodeConfig {
    pub condition_type: ConditionEvaluationType,
    pub expression: String,
    pub true_path: Option<String>,
    pub false_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ConditionEvaluationType {
    JavaScript,
    JsonPath,
    Simple,
}



#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCallNodeConfig {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub tool_id: i64,
    pub input_parameters: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StoreContextNodeConfig {
    pub context_data: String, // Textarea content for context to store
    pub use_llm: bool, // Toggle for using LLM instead of static data
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FetchContextNodeConfig {
    pub context_data: String, // Textarea content for context to fetch
    pub use_llm: bool, // Toggle for using LLM instead of static data
}

// Workflow execution models
#[derive(Debug, Serialize, Deserialize, Clone)]
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ExecutionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Timeout,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExecutionContext {
    pub variables: HashMap<String, serde_json::Value>,
    pub node_executions: Vec<NodeExecution>,
    pub current_node: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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
            enable_logging: true,
            enable_metrics: true,
            variables: HashMap::new(),
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


