use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgenticExecutionSender {
    User,
    Agent,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgenticExecutionMessage {
    pub sender: AgenticExecutionSender,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgenticExecutionState {
    Idle,
    Running,
    Interrupted,
    Completed,
}
