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
