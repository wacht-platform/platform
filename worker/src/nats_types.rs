use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsTaskMessage {
    pub task_type: String,
    pub task_id: String,
    pub payload: serde_json::Value,
    pub retry_count: u32,
    pub max_retries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
