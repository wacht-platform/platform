use std::sync::Arc;

use super::models::WebsocketMessage;
use serde_json::Value;
use shared::{models::AiAgentWithFeatures, state::AppState};
use tokio::sync::{Notify, mpsc};

#[derive(Clone)]
pub struct SessionState {
    pub sender: mpsc::UnboundedSender<WebsocketMessage<Value>>,
    pub deployment_id: i64,
    pub context_id: Option<i64>,
    pub agent: Option<AiAgentWithFeatures>,
    pub app_state: AppState,
    pub ready: Arc<Notify>,
    pub close: Arc<Notify>,
}

impl SessionState {
    pub fn new(
        sender: mpsc::UnboundedSender<WebsocketMessage<Value>>,
        app_state: AppState,
        deployment_id: i64,
    ) -> Self {
        Self {
            sender,
            deployment_id,
            context_id: None,
            agent: None,
            app_state,
            ready: Arc::new(Notify::new()),
            close: Arc::new(Notify::new()),
        }
    }
}
