use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Serialize, Deserialize, Clone)]
pub struct AgentExecutionContext {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub title: String,
    pub current_goal: String,
    pub tasks: Vec<String>,
    pub context_group: Option<String>,
    pub last_activity_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub execution_state: Option<AgentExecutionState>,
    pub status: ExecutionContextStatus,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub enum ExecutionContextStatus {
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "waiting_for_input")]
    WaitingForInput,
    #[serde(rename = "interrupted")]
    Interrupted,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
}

// Redis stream message structure
#[derive(Serialize, Deserialize, Clone)]
pub struct AgentStreamMessage {
    pub execution_context_id: i64,
    pub message_type: String,
    pub content: String,
    pub metadata: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AgentExecutionRequest {
    pub agent_name: String,
    pub user_input: String,
    pub session_id: Option<String>,
    pub execution_context_id: Option<i64>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AgentExecutionResponse {
    pub execution_context_id: i64,
    pub status: ExecutionContextStatus,
    pub message: String,
    pub metadata: serde_json::Value,
}

// Context engine operations
#[derive(Serialize, Deserialize, Clone)]
pub struct ContextStoreRequest {
    pub key: String,
    pub data: serde_json::Value,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ContextFetchRequest {
    pub key: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ContextSearchRequest {
    pub query: String,
    pub max_results: Option<usize>,
    pub metadata: Option<serde_json::Value>,
}

// Implementation helpers
impl ExecutionContextStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::WaitingForInput)
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

impl Default for ExecutionContextStatus {
    fn default() -> Self {
        Self::Idle
    }
}

// Agent execution state that can be restored
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentExecutionState {
    pub executable_tasks: Vec<serde_json::Value>,
    pub task_results: HashMap<String, serde_json::Value>,
    pub is_in_planning_mode: bool,
    pub current_objective: Option<serde_json::Value>,
    pub conversation_insights: Option<serde_json::Value>,
    pub workflow_state: Option<WorkflowExecutionState>,
    pub pending_input_request: Option<UserInputRequestState>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkflowExecutionState {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub workflow_id: i64,
    pub workflow_state: HashMap<String, serde_json::Value>,
    pub current_node_id: String,
    pub execution_path: Vec<String>, // Path of node IDs to reach current position
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserInputRequestState {
    pub question: String,
    pub context: String,
    pub input_type: String,
    pub options: Option<Vec<String>>,
    pub default_value: Option<String>,
    pub placeholder: Option<String>,
}

use std::collections::HashMap;

// String conversions for database storage
impl Display for ExecutionContextStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionContextStatus::Idle => write!(f, "idle"),
            ExecutionContextStatus::Running => write!(f, "running"),
            ExecutionContextStatus::WaitingForInput => write!(f, "waiting_for_input"),
            ExecutionContextStatus::Interrupted => write!(f, "interrupted"),
            ExecutionContextStatus::Completed => write!(f, "completed"),
            ExecutionContextStatus::Failed => write!(f, "failed"),
        }
    }
}

impl FromStr for ExecutionContextStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "idle" => Ok(ExecutionContextStatus::Idle),
            "running" => Ok(ExecutionContextStatus::Running),
            "waiting_for_input" => Ok(ExecutionContextStatus::WaitingForInput),
            "interrupted" => Ok(ExecutionContextStatus::Interrupted),
            "completed" => Ok(ExecutionContextStatus::Completed),
            "failed" => Ok(ExecutionContextStatus::Failed),
            _ => Err(()),
        }
    }
}

