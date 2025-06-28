use super::models::{ExecutionInfo, ExecutionStatus, WebsocketMessage};
use std::collections::HashMap;
use tokio::sync::mpsc;

pub struct SessionState {
    pub sender: mpsc::UnboundedSender<WebsocketMessage>,
    pub deployment_id: i64,
    pub session_id: Option<String>,
    pub active_executions: HashMap<u64, ExecutionInfo>,
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

impl SessionState {
    pub fn new(sender: mpsc::UnboundedSender<WebsocketMessage>, deployment_id: i64) -> Self {
        Self {
            sender,
            deployment_id,
            session_id: None,
            active_executions: HashMap::new(),
            last_activity: chrono::Utc::now(),
        }
    }

    pub fn add_execution(&mut self, execution_id: u64, agent_name: String) {
        let execution_info = ExecutionInfo::new(execution_id, agent_name);
        self.active_executions.insert(execution_id, execution_info);
        self.update_activity();
    }

    pub fn update_execution_status(&mut self, execution_id: u64, status: ExecutionStatus) {
        if let Some(execution) = self.active_executions.get_mut(&execution_id) {
            execution.update_status(status);
        }
        self.update_activity();
    }

    pub fn remove_execution(&mut self, execution_id: u64) {
        self.active_executions.remove(&execution_id);
        self.update_activity();
    }

    pub fn update_activity(&mut self) {
        self.last_activity = chrono::Utc::now();
    }
}
