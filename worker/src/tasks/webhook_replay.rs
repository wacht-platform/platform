use commands::webhook_trigger::ReplayWebhookDeliveryCommand;
use common::error::AppError;
use common::state::AppState;

pub async fn replay_webhook_delivery(
    app_state: &AppState,
    delivery_id: i64,
    deployment_id: i64,
) -> Result<i64, AppError> {
    ReplayWebhookDeliveryCommand {
        delivery_id,
        deployment_id,
    }
    .execute_with_deps(&common::deps::from_app(app_state).db().nats().id())
    .await
}
