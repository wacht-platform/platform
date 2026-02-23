use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

/// Status update in an agent's execution timeline
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentStatusUpdate {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub context_id: i64,
    pub status_update: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AgentExecutionContext {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub title: String,
    pub system_instructions: Option<String>,
    pub context_group: Option<String>,
    pub last_activity_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub execution_state: Option<AgentExecutionState>,
    pub status: ExecutionContextStatus,
    pub source: Option<String>,
    pub external_context_id: Option<String>,
    pub external_resource_metadata: Option<serde_json::Value>,
    /// Parent context if this agent was spawned by another
    pub parent_context_id: Option<i64>,
    /// Completion summary stored when child agent finishes
    pub completion_summary: Option<serde_json::Value>,
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
    pub task_results: HashMap<String, serde_json::Value>,
    pub current_objective: Option<serde_json::Value>,
    pub conversation_insights: Option<serde_json::Value>,
    #[serde(default)]
    pub supervisor_mode_active: bool,
    #[serde(default)]
    pub supervisor_task_board: Vec<serde_json::Value>,
    #[serde(default)]
    pub deep_think_mode_active: bool,
    #[serde(default)]
    pub deep_think_used: usize,
    pub pending_input_request: Option<UserInputRequestState>,
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
