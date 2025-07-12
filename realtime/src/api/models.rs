use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, Clone)]
pub enum WebsocketMessageType {
    #[serde(rename = "fetch_context_messages")]
    FetchContextMessages,
    #[serde(rename = "request_context_response")]
    RequestContextResponse(u64),
    #[serde(rename = "message_input")]
    MessageInput(String),
    #[serde(rename = "new_message_chunk")]
    NewMessageChunk,
    #[serde(rename = "execution_complete")]
    ExecutionComplete(u64),
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
    #[serde(rename = "session_connect")]
    SessionConnect(String, String),
    #[serde(rename = "session_connected")]
    SessionConnected,
    #[serde(rename = "session_status")]
    SessionStatus(String),
    #[serde(rename = "close_connection")]
    CloseConnection,
    #[serde(rename = "platform_event")]
    PlatformEvent,
    #[serde(rename = "platform_function")]
    PlatformFunction,
    #[serde(rename = "platform_function_result")]
    PlatformFunctionResult(u64, serde_json::Value),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct WebsocketMessage<T> {
    pub message_id: Option<u64>,
    pub message_type: WebsocketMessageType,
    pub data: T,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Starting,
    Running,
    WaitingForInput,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ExecutionInfo {
    pub execution_id: u64,
    pub agent_name: String,
    pub status: ExecutionStatus,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub last_update: chrono::DateTime<chrono::Utc>,
    pub task_count: u32,
    pub completed_tasks: u32,
}
