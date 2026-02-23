use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct NatsTaskMessage {
    pub task_type: String,
    pub task_id: String,
    pub payload: serde_json::Value,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiKeyOrgMembershipSyncPayload {
    pub membership_id: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiKeyWorkspaceMembershipSyncPayload {
    pub membership_id: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiKeyOrgRoleSyncPayload {
    pub role_id: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiKeyWorkspaceRoleSyncPayload {
    pub role_id: i64,
}

// Webhook replay batch task payloads
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebhookReplayBatchPayload {
    #[serde(rename = "by_ids")]
    ByIds {
        deployment_id: String,
        app_slug: String,
        delivery_ids: Vec<String>,
    },
    #[serde(rename = "by_date_range")]
    ByDateRange {
        deployment_id: String,
        app_slug: String,
        start_date: DateTime<Utc>,
        end_date: Option<DateTime<Utc>>,
        status: Option<String>,
        event_name: Option<String>,
        endpoint_id: Option<i64>,
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
    #[serde(rename = "new_message")]
    NewMessage { conversation_id: String },

    #[serde(rename = "platform_function_result")]
    PlatformFunctionResult {
        execution_id: String,
        result: serde_json::Value,
    },

    #[serde(rename = "user_input_response")]
    UserInputResponse { conversation_id: String },
}

/// Request to execute an agent via NATS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExecutionRequest {
    pub deployment_id: String,
    pub context_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(flatten)]
    pub execution_type: AgentExecutionType,
}
