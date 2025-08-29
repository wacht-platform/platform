pub mod executor;
pub mod context;
pub mod tools;
pub mod gemini;
pub mod template;
pub mod handler;

pub use executor::{AgentExecutor, AgentExecutorBuilder, ResumeContext};
pub use context::ContextOrchestrator;
pub use tools::ToolExecutor;
pub use gemini::GeminiClient;
pub use handler::{AgentHandler, ExecutionRequest};

use dto::json::StreamEvent;
use tokio::sync::mpsc;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub enum StreamChannel {
    Nats(mpsc::Sender<StreamEvent>),
    Memory(Arc<Mutex<Vec<StreamEvent>>>),
}

impl StreamChannel {
    pub async fn send(&self, event: StreamEvent) -> Result<(), String> {
        match self {
            StreamChannel::Nats(sender) => {
                sender.send(event).await
                    .map_err(|e| format!("Failed to send to NATS channel: {}", e))
            }
            StreamChannel::Memory(events) => {
                events.lock()
                    .map_err(|e| format!("Failed to lock memory channel: {}", e))?
                    .push(event);
                Ok(())
            }
        }
    }
}