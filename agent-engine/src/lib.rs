pub mod clickup;
pub mod context;
pub mod execution_context;
pub mod executor;
pub mod filesystem;
pub mod gemini;
pub mod handler;
pub mod swarm;
pub mod template;
pub mod tools;

pub use context::ContextOrchestrator;
pub use execution_context::ExecutionContext;
pub use executor::{AgentExecutor, ResumeContext};
pub use gemini::GeminiClient;
pub use handler::{AgentHandler, ExecutionRequest};
pub use tools::ToolExecutor;

use dto::json::StreamEvent;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Clone)]
pub enum StreamChannel {
    Nats(mpsc::Sender<StreamEvent>),
    Memory(Arc<Mutex<Vec<StreamEvent>>>),
}

impl StreamChannel {
    pub async fn send(&self, event: StreamEvent) -> Result<(), String> {
        match self {
            StreamChannel::Nats(sender) => sender
                .send(event)
                .await
                .map_err(|e| format!("Failed to send to NATS channel: {}", e)),
            StreamChannel::Memory(events) => {
                events
                    .lock()
                    .map_err(|e| format!("Failed to lock memory channel: {}", e))?
                    .push(event);
                Ok(())
            }
        }
    }
}
