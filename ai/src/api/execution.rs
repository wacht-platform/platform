use serde::{Deserialize, Serialize};

/// Sender type for agentic execution messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgenticExecutionSender {
    User,
    Agent,
    System,
}

/// Message in an agentic execution context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgenticExecutionMessage {
    pub sender: AgenticExecutionSender,
    pub data: Vec<u8>,
}

/// State of an agentic execution
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgenticExecutionState {
    Idle,
    Running,
    Interrupted,
    Completed,
}
