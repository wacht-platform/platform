use anyhow::Result;
use commands::webhook_trigger::ReplayWebhookDeliveryCommand;
use commands::Command;
use common::state::AppState;
use dto::json::nats::WebhookReplayBatchPayload;
use serde_json::Value;
use tracing::{error, info};

pub async fn handle_webhook_replay_batch(
    app_state: &AppState,
    payload: Value,
) -> Result<String> {
    // Deserialize the payload into our strongly typed DTO
    let replay_payload: WebhookReplayBatchPayload = serde_json::from_value(payload)
        .map_err(|e| anyhow::anyhow!("Failed to deserialize webhook replay payload: {}", e))?;
    
    let (deployment_id, delivery_ids, _include_successful) = match replay_payload {
        WebhookReplayBatchPayload::ByIds { deployment_id, delivery_ids, include_successful } => {
            // Parse delivery IDs from strings to i64
            let ids: Vec<i64> = delivery_ids
                .iter()
                .filter_map(|s| s.parse::<i64>().ok())
                .collect();
            
            if ids.len() != delivery_ids.len() {
                error!("Some delivery IDs failed to parse. Original: {:?}, Parsed: {:?}", delivery_ids, ids);
            }
            
            // Validate these IDs exist and optionally filter successful ones
            let validated_ids = app_state.clickhouse_service
                .get_deliveries_by_ids(deployment_id, ids, include_successful)
                .await?;
            
            (deployment_id, validated_ids, include_successful)
        }
        WebhookReplayBatchPayload::ByDateRange { deployment_id, start_date, end_date, include_successful } => {
            let ids = app_state.clickhouse_service
                .get_deliveries_for_replay(deployment_id, start_date, end_date, include_successful)
                .await?;
            
            (deployment_id, ids, include_successful)
        }
    };
    
    if delivery_ids.is_empty() {
        info!("No deliveries found to replay");
        return Ok("No deliveries found to replay".to_string());
    }
    
    info!(
        "Found {} deliveries to replay for deployment {}",
        delivery_ids.len(),
        deployment_id
    );
    
    let mut replayed_count = 0;
    let mut failed_count = 0;
    
    // Process each delivery
    for delivery_id in delivery_ids {
        let result = ReplayWebhookDeliveryCommand {
            delivery_id,
            deployment_id,
        }
        .execute(app_state)
        .await;
        
        match result {
            Ok(new_id) => {
                info!(
                    "Successfully replayed delivery {} as new delivery {}",
                    delivery_id, new_id
                );
                replayed_count += 1;
            }
            Err(e) => {
                error!("Failed to replay delivery {}: {}", delivery_id, e);
                failed_count += 1;
            }
        }
    }
    
    Ok(format!(
        "Replay batch completed: {} successful, {} failed",
        replayed_count, failed_count
    ))
}