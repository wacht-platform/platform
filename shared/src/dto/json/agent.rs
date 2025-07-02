use crate::models::AgentExecutionContextMessage;

pub enum StreamEvent {
    Token(String),
    Message(AgentExecutionContextMessage),
}
