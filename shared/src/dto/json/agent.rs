use crate::models::AgentExecutionContextMessage;

pub enum StreamEvent {
    Token(String, String),
    Message(AgentExecutionContextMessage),
}
