use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct NatsTaskMessage {
    pub task_type: String,
    pub task_id: String,
    pub payload: serde_json::Value,
}

// Webhook replay batch task payloads
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebhookReplayBatchPayload {
    #[serde(rename = "by_ids")]
    ByIds {
        deployment_id: i64,
        delivery_ids: Vec<String>,
        include_successful: bool,
    },
    #[serde(rename = "by_date_range")]
    ByDateRange {
        deployment_id: i64,
        start_date: DateTime<Utc>,
        end_date: Option<DateTime<Utc>>,
        include_successful: bool,
    },
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub success: bool,
    pub result: Option<String>,
    pub error: Option<String>,
}

impl TaskResult {
    pub fn success(task_id: String, result: String) -> Self {
        Self {
            task_id,
            success: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(task_id: String, error: String) -> Self {
        Self {
            task_id,
            success: false,
            result: None,
            error: Some(error),
        }
    }
}

/// Type of agent execution request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentExecutionType {
    /// New message from user - conversation already persisted
    #[serde(rename = "new_message")]
    NewMessage { conversation_id: i64 },
    
    /// Platform function result - worker will persist this
    #[serde(rename = "platform_function_result")]
    PlatformFunctionResult { execution_id: String, result: serde_json::Value },
    
    /// User input response - conversation already persisted
    #[serde(rename = "user_input_response")]
    UserInputResponse { conversation_id: i64 },
}

/// Request to execute an agent via NATS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExecutionRequest {
    pub deployment_id: i64,
    pub context_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(flatten)]
    pub execution_type: AgentExecutionType,
}

