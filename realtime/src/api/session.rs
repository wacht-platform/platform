use super::models::WebsocketMessage;
use serde_json::Value;
use shared::models::AiAgent;
use tokio::sync::mpsc;

pub struct SessionState {
    pub sender: mpsc::UnboundedSender<WebsocketMessage<Value>>,
    pub deployment_id: i64,
    pub context_id: Option<i64>,
    pub agent: Option<AiAgent>,
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

impl SessionState {
    pub fn new(sender: mpsc::UnboundedSender<WebsocketMessage<Value>>, deployment_id: i64) -> Self {
        Self {
            sender,
            deployment_id,
            context_id: None,
            agent: None,
            last_activity: chrono::Utc::now(),
        }
    }

    pub fn update_activity(&mut self) {
        self.last_activity = chrono::Utc::now();
    }
}
