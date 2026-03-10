use common::{HasNatsProvider, error::AppError};
use dto::json::nats::NatsTaskMessage;

pub struct EnqueueWebhookDeliveryCommand {
    task_id: String,
    delivery_id: i64,
    deployment_id: i64,
}

impl EnqueueWebhookDeliveryCommand {
    pub fn new(task_id: String, delivery_id: i64, deployment_id: i64) -> Self {
        Self {
            task_id,
            delivery_id,
            deployment_id,
        }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasNatsProvider + ?Sized,
    {
        let task_message = NatsTaskMessage {
            task_type: "webhook.deliver".to_string(),
            task_id: self.task_id,
            payload: serde_json::json!({
                "delivery_id": self.delivery_id,
                "deployment_id": self.deployment_id
            }),
        };

        deps.nats_provider()
            .publish(
                "worker.tasks.webhook.deliver",
                serde_json::to_vec(&task_message)
                    .map_err(|e| AppError::Internal(format!("Failed to serialize task: {}", e)))?
                    .into(),
            )
            .await
            .map_err(|e| AppError::Internal(format!("Failed to publish to NATS: {}", e)))?;

        Ok(())
    }
}
