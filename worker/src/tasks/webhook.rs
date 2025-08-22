use anyhow::Result;
use chrono::Utc;
use commands::{
    Command,
    webhook_delivery::{
        ClearEndpointFailuresCommand, DeactivateEndpointCommand, DeleteActiveDeliveryCommand,
        GetActiveDeliveryCommand, IncrementEndpointFailuresCommand, UpdateDeliveryAttemptsCommand,
        calculate_next_retry,
    },
    webhook_storage::RetrieveWebhookPayloadCommand,
};
use common::state::AppState;
use common::utils::webhook;
use dto::clickhouse::webhook::WebhookDelivery;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{error, info, warn};

#[derive(Debug, Deserialize, Serialize)]
pub struct WebhookDeliveryTask {
    pub delivery_id: i64,
    pub deployment_id: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WebhookBatchDeliveryTask {
    pub delivery_ids: Vec<i64>,
    pub deployment_id: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WebhookRetryTask {
    pub delivery_id: i64,
    pub deployment_id: i64,
}

#[derive(Debug)]
pub enum DeliveryResult {
    Success,
    Failed,
    NotFound,
    RetryAfter(std::time::Duration), // Add retry with delay
}

// HTTP status codes we need
const STATUS_REQUEST_TIMEOUT: u16 = 408;
const STATUS_TOO_MANY_REQUESTS: u16 = 429;
const STATUS_INTERNAL_SERVER_ERROR: u16 = 500;

pub async fn process_webhook_delivery(
    delivery_id: i64,
    deployment_id: i64,
    app_state: &AppState,
) -> Result<DeliveryResult> {
    // Get delivery details using command
    let command = GetActiveDeliveryCommand { delivery_id };
    let delivery = match command.execute(app_state).await? {
        Some(d) => d,
        None => {
            warn!("Webhook delivery {} not found", delivery_id);
            return Ok(DeliveryResult::NotFound);
        }
    };

    info!(
        "Processing webhook delivery {} (attempt {}/{})",
        delivery_id,
        delivery.attempts + 1,
        delivery.max_attempts
    );

    // Retrieve payload from S3
    let payload = RetrieveWebhookPayloadCommand::new(delivery.payload_s3_key.clone())
        .execute(app_state)
        .await?;

    // Build HTTP request using reqwest from shared dependencies
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            delivery.timeout_seconds as u64,
        ))
        .user_agent("Wacht-Webhook/1.0")
        .build()?;

    let mut request = client.post(&delivery.url).json(&payload);

    // Generate and add signature header
    let signature = webhook::generate_hmac_signature(&delivery.signing_secret, &payload);

    request = request
        .header("X-Webhook-Signature", signature)
        .header("X-Webhook-Event", &delivery.event_name)
        .header("X-Webhook-Delivery", delivery_id.to_string());

    // Add custom headers
    if let Some(headers) = &delivery.headers {
        if let Some(headers_obj) = headers.as_object() {
            for (key, value) in headers_obj {
                if let Some(value_str) = value.as_str() {
                    request = request.header(key, value_str);
                }
            }
        }
    }

    // Make the request
    let start = Instant::now();
    let result = request.send().await;
    let duration = start.elapsed();

    match result {
        Ok(response) => {
            let status = response.status();
            let status_code = status.as_u16();
            let response_body = response.text().await.ok();

            if status.is_success() {
                info!(
                    "Successfully delivered webhook {} to {} ({}ms)",
                    delivery_id,
                    delivery.url,
                    duration.as_millis()
                );

                // Log success to ClickHouse with payload
                let ch_delivery = WebhookDelivery {
                    deployment_id,
                    delivery_id,

                    app_name: delivery.app_name.clone(),
                    endpoint_id: delivery.endpoint_id,
                    endpoint_url: delivery.url.clone(),
                    event_name: delivery.event_name.clone(),
                    status: "success".to_string(),
                    http_status_code: Some(status_code as i32),
                    response_time_ms: Some(duration.as_millis() as i32),
                    attempt_number: delivery.attempts + 1,
                    max_attempts: delivery.max_attempts,
                    error_message: None,
                    filtered_reason: None,
                    payload_s3_key: delivery.payload_s3_key.clone(),
                    response_body: response_body.clone(),
                    response_headers: None,
                    timestamp: Utc::now(),
                };

                if let Err(e) = app_state
                    .clickhouse_service
                    .insert_webhook_delivery(&ch_delivery)
                    .await
                {
                    warn!("Failed to log successful delivery to ClickHouse: {}", e);
                }

                // Delete from active queue using command
                DeleteActiveDeliveryCommand { delivery_id }
                    .execute(app_state)
                    .await?;

                Ok(DeliveryResult::Success)
            } else {
                warn!(
                    "Webhook {} returned non-success status: {}",
                    delivery_id, status
                );

                // Check if this will be retried
                let will_retry = (delivery.attempts + 1) < delivery.max_attempts
                    && (status_code >= 500 || status_code == 408 || status_code == 429);

                // Log failure to ClickHouse with payload
                let ch_delivery = WebhookDelivery {
                    deployment_id,
                    delivery_id,

                    app_name: delivery.app_name.clone(),
                    endpoint_id: delivery.endpoint_id,
                    endpoint_url: delivery.url.clone(),
                    event_name: delivery.event_name.clone(),
                    status: if will_retry {
                        "failed".to_string()
                    } else {
                        "permanently_failed".to_string()
                    },
                    http_status_code: Some(status_code as i32),
                    response_time_ms: Some(duration.as_millis() as i32),
                    attempt_number: delivery.attempts + 1,
                    max_attempts: delivery.max_attempts,
                    error_message: None,
                    filtered_reason: None,
                    payload_s3_key: delivery.payload_s3_key.clone(),
                    response_body: response_body.clone(),
                    response_headers: None,
                    timestamp: Utc::now(),
                };

                if let Err(e) = app_state
                    .clickhouse_service
                    .insert_webhook_delivery(&ch_delivery)
                    .await
                {
                    warn!("Failed to log failed delivery to ClickHouse: {}", e);
                }

                let result = handle_delivery_failure(
                    delivery_id,
                    deployment_id,
                    delivery.app_name.clone(),
                    delivery.endpoint_id,
                    delivery.url.clone(),
                    delivery.attempts + 1,
                    delivery.max_attempts,
                    Some(status_code),
                    app_state,
                )
                .await?;

                Ok(result)
            }
        }
        Err(e) => {
            error!("Failed to deliver webhook {}: {}", delivery_id, e);

            // Network errors are typically retryable
            let will_retry = (delivery.attempts + 1) < delivery.max_attempts;

            // Log error to ClickHouse with payload
            let ch_delivery = WebhookDelivery {
                deployment_id,
                delivery_id,

                app_name: delivery.app_name.clone(),
                endpoint_id: delivery.endpoint_id,
                endpoint_url: delivery.url.clone(),
                event_name: delivery.event_name.clone(),
                status: if will_retry {
                    "failed".to_string()
                } else {
                    "permanently_failed".to_string()
                },
                http_status_code: None,
                response_time_ms: None,
                attempt_number: delivery.attempts + 1,
                max_attempts: delivery.max_attempts,
                error_message: Some(e.to_string()),
                filtered_reason: None,
                payload_s3_key: delivery.payload_s3_key.clone(),
                response_body: None,
                response_headers: None,
                timestamp: Utc::now(),
            };

            if let Err(e) = app_state
                .clickhouse_service
                .insert_webhook_delivery(&ch_delivery)
                .await
            {
                warn!("Failed to log error delivery to ClickHouse: {}", e);
            }

            let result = handle_delivery_failure(
                delivery_id,
                deployment_id,
                delivery.app_name,
                delivery.endpoint_id,
                delivery.url,
                delivery.attempts + 1,
                delivery.max_attempts,
                None,
                app_state,
            )
            .await?;

            Ok(result)
        }
    }
}

async fn handle_delivery_failure(
    delivery_id: i64,
    deployment_id: i64,
    app_name: String,
    endpoint_id: i64,
    endpoint_url: String,
    new_attempts: i32,
    max_attempts: i32,
    status_code: Option<u16>,
    app_state: &AppState,
) -> Result<DeliveryResult> {
    // Check if we should retry
    let should_retry = new_attempts < max_attempts
        && status_code.map_or(true, |s| {
            // Retry on 5xx errors and timeouts
            s >= STATUS_INTERNAL_SERVER_ERROR
                || s == STATUS_REQUEST_TIMEOUT
                || s == STATUS_TOO_MANY_REQUESTS
        });

    if should_retry {
        // Calculate next retry time with exponential backoff
        let next_retry = calculate_next_retry(new_attempts);
        let retry_delay = (next_retry - Utc::now()).num_seconds().max(1) as u64;

        info!(
            "Scheduling retry for delivery {} at {} (delay: {}s)",
            delivery_id, next_retry, retry_delay
        );

        // Update attempts in database
        UpdateDeliveryAttemptsCommand {
            delivery_id,
            new_attempts,
            next_retry_at: next_retry,
        }
        .execute(app_state)
        .await?;

        // Return retry delay so consumer can NAK with delay
        return Ok(DeliveryResult::RetryAfter(std::time::Duration::from_secs(
            retry_delay,
        )));
    } else {
        warn!(
            "Max attempts reached for delivery {} or non-retryable error",
            delivery_id
        );

        // Delete from active queue using command
        DeleteActiveDeliveryCommand { delivery_id }
            .execute(app_state)
            .await?;

        // Check if we should auto-deactivate the endpoint using command
        if new_attempts >= max_attempts {
            let failure_count = IncrementEndpointFailuresCommand { endpoint_id }
                .execute(app_state)
                .await?;

            // Auto-deactivate if threshold reached
            const DEACTIVATION_THRESHOLD: i64 = 10;
            if failure_count >= DEACTIVATION_THRESHOLD {
                warn!(
                    "Auto-deactivating endpoint {} after {} max-attempt failures in 24 hours",
                    endpoint_id, failure_count
                );

                // Deactivate the endpoint using command
                DeactivateEndpointCommand { endpoint_id }
                    .execute(app_state)
                    .await?;

                // Clear the failure counter using command
                ClearEndpointFailuresCommand { endpoint_id }
                    .execute(app_state)
                    .await?;

                // Log deactivation event to ClickHouse
                let ch_delivery = WebhookDelivery {
                    deployment_id,
                    delivery_id: app_state.sf.next_id().unwrap() as i64,

                    app_name: app_name.clone(),
                    endpoint_id,
                    endpoint_url: endpoint_url.clone(),
                    event_name: "endpoint.deactivated".to_string(),
                    status: "deactivated".to_string(),
                    http_status_code: None,
                    response_time_ms: None,
                    attempt_number: 0,
                    max_attempts: 0,
                    error_message: Some(format!(
                        "Auto-deactivated after {} max-attempt failures",
                        DEACTIVATION_THRESHOLD
                    )),
                    filtered_reason: None,
                    payload_s3_key: "endpoint-deactivation".to_string(),
                    response_body: None,
                    response_headers: None,
                    timestamp: Utc::now(),
                };

                if let Err(e) = app_state
                    .clickhouse_service
                    .insert_webhook_delivery(&ch_delivery)
                    .await
                {
                    warn!("Failed to log endpoint deactivation to ClickHouse: {}", e);
                }

                // TODO: Send notification to customer about endpoint deactivation
                info!(
                    "Endpoint {} for app {} has been auto-deactivated. Customer should be notified.",
                    endpoint_id, app_name
                );
            }
        }

        // Don't log to ClickHouse here - already logged with correct status in the main handler
    }

    Ok(DeliveryResult::Failed)
}

// Batch processing for efficiency
pub async fn process_webhook_batch(
    delivery_ids: Vec<i64>,
    deployment_id: i64,
    app_state: &AppState,
) -> Result<String> {
    if delivery_ids.is_empty() {
        return Ok("No deliveries to process".to_string());
    }

    info!(
        "Processing webhook batch of {} deliveries for deployment {}",
        delivery_ids.len(),
        deployment_id
    );

    let mut successful = 0;
    let mut failed = 0;
    let mut not_found = 0;

    // Process deliveries in parallel chunks
    const PARALLEL_LIMIT: usize = 50;

    for chunk in delivery_ids.chunks(PARALLEL_LIMIT) {
        let mut handles = Vec::new();

        for &delivery_id in chunk {
            let app_state_clone = app_state.clone();
            let handle = tokio::spawn(async move {
                process_webhook_delivery(delivery_id, deployment_id, &app_state_clone).await
            });
            handles.push(handle);
        }

        // Wait for chunk to complete
        for handle in handles {
            match handle.await {
                Ok(Ok(result)) => match result {
                    DeliveryResult::Success => successful += 1,
                    DeliveryResult::Failed => failed += 1,
                    DeliveryResult::NotFound => not_found += 1,
                    DeliveryResult::RetryAfter(_) => failed += 1, // Count as failed for batch stats
                },
                Ok(Err(e)) => {
                    error!("Webhook delivery error: {}", e);
                    failed += 1;
                }
                Err(e) => {
                    error!("Task join error: {}", e);
                    failed += 1;
                }
            }
        }
    }

    Ok(format!(
        "Batch processed: {} successful, {} failed, {} not found",
        successful, failed, not_found
    ))
}

pub async fn process_webhook_retry(
    delivery_id: i64,
    deployment_id: i64,
    app_state: &AppState,
) -> Result<String> {
    use commands::webhook_trigger::ReplayWebhookDeliveryCommand;

    info!(
        "Processing webhook retry for delivery {} in deployment {}",
        delivery_id, deployment_id
    );

    // Execute the replay command which handles all the logic
    let new_delivery_id = ReplayWebhookDeliveryCommand {
        delivery_id,
        deployment_id,
    }
    .execute(app_state)
    .await
    .map_err(|e| anyhow::anyhow!("Failed to replay webhook delivery: {}", e))?;

    Ok(format!(
        "Webhook delivery {} retried as new delivery {}",
        delivery_id, new_delivery_id
    ))
}
