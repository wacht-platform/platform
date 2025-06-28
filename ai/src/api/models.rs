use serde::{Deserialize, Serialize};

/// WebSocket message types for AI agent communication
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub enum WebsocketMessageType {
    #[serde(rename = "request_context")]
    RequestContext(String), // Just agent_name, returns current execution context
    #[serde(rename = "request_context_response")]
    RequestContextResponse(u64),
    #[serde(rename = "message_input")]
    MessageInput(u64, String), // execution_id, user_message
    #[serde(rename = "execution_update")]
    ExecutionUpdate(u64),
    #[serde(rename = "execution_complete")]
    ExecutionComplete(u64),
    #[serde(rename = "execution_input")]
    ExecutionInput(u64),
    #[serde(rename = "execution_interrupt")]
    ExecutionInterrupt(u64),
    #[serde(rename = "execution_error")]
    ExecutionError(u64),
    #[serde(rename = "task_update")]
    TaskUpdate(u64),
    #[serde(rename = "reasoning_update")]
    ReasoningUpdate(u64),
    #[serde(rename = "tool_execution")]
    ToolExecution(u64),
    #[serde(rename = "workflow_execution")]
    WorkflowExecution(u64),
    #[serde(rename = "session_reconnect")]
    SessionReconnect(String),
    #[serde(rename = "session_status")]
    SessionStatus(String),
    #[serde(rename = "close_connection")]
    CloseConnection,
}

/// WebSocket message structure
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WebsocketMessage {
    pub message_type: WebsocketMessageType,
    pub data: Vec<u8>,
}

/// Execution status for agent operations
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Starting,
    Running,
    WaitingForInput,
    Completed,
    Failed,
    Interrupted,
}

/// Information about an active execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionInfo {
    pub execution_id: u64,
    pub agent_name: String,
    pub status: ExecutionStatus,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub last_update: chrono::DateTime<chrono::Utc>,
    pub task_count: u32,
    pub completed_tasks: u32,
}

impl ExecutionInfo {
    pub fn new(execution_id: u64, agent_name: String) -> Self {
        let now = chrono::Utc::now();
        Self {
            execution_id,
            agent_name,
            status: ExecutionStatus::Starting,
            started_at: now,
            last_update: now,
            task_count: 0,
            completed_tasks: 0,
        }
    }

    pub fn update_status(&mut self, status: ExecutionStatus) {
        self.status = status;
        self.last_update = chrono::Utc::now();
    }
}
