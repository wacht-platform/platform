use std::sync::Arc;

use super::models::WebsocketMessage;
use common::state::AppState;
use models::AiAgentWithFeatures;
use serde_json::Value;
use tokio::sync::{Notify, mpsc};

#[derive(Clone)]
pub struct SessionState {
    pub sender: mpsc::UnboundedSender<WebsocketMessage<Value>>,
    pub deployment_id: i64,
    pub user_id: Option<String>,
    pub context_id: Option<i64>,
    pub audience: Option<String>,
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
            user_id: None,
            context_id: None,
            audience: None,
            agent: None,
            app_state,
            ready: Arc::new(Notify::new()),
            close: Arc::new(Notify::new()),
        }
    }

    pub fn with_user(mut self, user_id: Option<String>) -> Self {
        self.user_id = user_id;
        self
    }

    pub fn with_audience(mut self, audience: Option<String>) -> Self {
        self.audience = audience;
        self
    }
}
