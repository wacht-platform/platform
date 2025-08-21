use crate::Command;
use common::error::AppError;
use common::state::AppState;
use dto::json::nats::NatsTaskMessage;
use serde_json;

pub struct DispatchDocumentBatchTaskCommand {
    pub deployment_id: i64,
    pub knowledge_base_id: i64,
    pub batch_size: usize,
}

impl DispatchDocumentBatchTaskCommand {
    pub fn new(deployment_id: i64, knowledge_base_id: i64, batch_size: usize) -> Self {
        Self {
            deployment_id,
            knowledge_base_id,
            batch_size,
        }
    }
}

impl Command for DispatchDocumentBatchTaskCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let task_message = NatsTaskMessage {
            task_type: "embedding.process_batch".to_string(),
            task_id: format!(
                "embedding-batch-{}-{}",
                self.deployment_id, self.knowledge_base_id
            ),
            payload: serde_json::json!({
                "deployment_id": self.deployment_id,
                "knowledge_base_id": self.knowledge_base_id,
                "batch_size": self.batch_size
            }),
        };

        app_state
            .nats_client
            .publish(
                "worker.tasks.embedding.process_batch",
                serde_json::to_vec(&task_message)
                    .map_err(|e| AppError::Internal(format!("Failed to serialize task: {}", e)))?
                    .into(),
            )
            .await
            .map_err(|e| {
                AppError::Internal(format!(
                    "Failed to publish embedding batch task to NATS: {}",
                    e
                ))
            })?;

        Ok(())
    }
}
