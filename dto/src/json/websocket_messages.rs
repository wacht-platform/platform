use serde::{Deserialize, Serialize};
use serde_json::Value;

// WebSocket Error Messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketError {
    pub error: String,
}

// Session Messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConnectedMessage {
    pub context: Value,
    pub execution_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quick_questions: Option<Vec<String>>,
}

// Execution Status Messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStatusUpdate {
    pub status: String,
}

// Platform Event/Function NATS Payloads
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformEventPayload {
    pub event_label: String,
    pub event_data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformFunctionPayload {
    pub function_name: String,
    pub function_data: Value,
}
