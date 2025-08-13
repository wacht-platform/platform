use anyhow::Result;
use chrono::Utc;
use commands::{
    Command,
    webhook_delivery::{
        ClearEndpointFailuresCommand, DeactivateEndpointCommand, DeleteActiveDeliveryCommand,
        GetActiveDeliveryCommand, GetFailedDeliveryDetailsCommand,
        IncrementEndpointFailuresCommand, UpdateDeliveryAttemptsCommand, calculate_next_retry,
    },
    webhook_storage::{RetrieveWebhookPayloadCommand, StoreFailedWebhookDeliveryCommand},
};
use common::clickhouse_webhook::WebhookDelivery;
use common::state::AppState;
use common::utils::webhook;
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

#[derive(Debug)]
pub enum DeliveryResult {
    Success,
    Failed,
    Blocked,
    NotFound,
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

    // Check IP allowlist if configured
    if let Some(ref allowlist_value) = delivery.ip_allowlist {
        if let Some(allowlist) = allowlist_value.as_array() {
            let allowlist_strings: Vec<String> = allowlist
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();

            if !allowlist_strings.is_empty() {
                // Resolve the endpoint URL to IPs
                match webhook::resolve_url_to_ips(&delivery.url).await {
                    Ok(ips) => {
                        // Check if any resolved IP is in the allowlist
                        let allowed = ips
                            .iter()
                            .any(|ip| webhook::is_ip_allowed(ip, &allowlist_strings));

                        if !allowed {
                            warn!(
                                "Webhook delivery {} blocked: endpoint IPs {:?} not in allowlist",
                                delivery_id, ips
                            );

                            // Log blocked delivery to ClickHouse
                            let ch_delivery = WebhookDelivery {
                                deployment_id,
                                delivery_id,
                                app_id: delivery.app_id,
                                app_name: delivery.app_name.clone(),
                                endpoint_id: delivery.endpoint_id,
                                endpoint_url: delivery.url.clone(),
                                event_name: delivery.event_name.clone(),
                                status: "blocked".to_string(),
                                http_status_code: None,
                                response_time_ms: None,
                                attempt_number: delivery.attempts + 1,
                                error_message: Some(format!(
                                    "IP not in allowlist. Resolved IPs: {:?}",
                                    ips
                                )),
                                filtered_reason: Some("ip_allowlist".to_string()),
                                timestamp: Utc::now(),
                            };

                            if let Err(e) = app_state
                                .clickhouse_service
                                .insert_webhook_delivery(&ch_delivery)
                                .await
                            {
                                warn!("Failed to log blocked delivery to ClickHouse: {}", e);
                            }

                            // Delete from active queue
                            DeleteActiveDeliveryCommand { delivery_id }
                                .execute(app_state)
                                .await?;

                            return Ok(DeliveryResult::Blocked);
                        }
                    }
                    Err(e) => {
                        error!("Failed to resolve endpoint URL {}: {}", delivery.url, e);

                        // Log DNS failure to ClickHouse
                        let ch_delivery = WebhookDelivery {
                            deployment_id,
                            delivery_id,
                            app_id: delivery.app_id,
                            app_name: delivery.app_name.clone(),
                            endpoint_id: delivery.endpoint_id,
                            endpoint_url: delivery.url.clone(),
                            event_name: delivery.event_name.clone(),
                            status: "failed".to_string(),
                            http_status_code: None,
                            response_time_ms: None,
                            attempt_number: delivery.attempts + 1,
                            error_message: Some(format!("DNS resolution failed: {}", e)),
                            filtered_reason: None,
                            timestamp: Utc::now(),
                        };

                        if let Err(e) = app_state
                            .clickhouse_service
                            .insert_webhook_delivery(&ch_delivery)
                            .await
                        {
                            warn!("Failed to log DNS failure to ClickHouse: {}", e);
                        }

                        // Handle as a failure (will retry if applicable)
                        handle_delivery_failure(
                            delivery_id,
                            deployment_id,
                            delivery.app_id,
                            delivery.app_name.clone(),
                            delivery.endpoint_id,
                            delivery.url.clone(),
                            delivery.event_name.clone(),
                            delivery.attempts + 1,
                            delivery.max_attempts,
                            None,
                            Some(format!("DNS resolution failed: {}", e)),
                            app_state,
                        )
                        .await?;

                        return Ok(DeliveryResult::Failed);
                    }
                }
            }
        }
    }

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

                // Log success to ClickHouse
                let ch_delivery = WebhookDelivery {
                    deployment_id,
                    delivery_id,
                    app_id: delivery.app_id,
                    app_name: delivery.app_name.clone(),
                    endpoint_id: delivery.endpoint_id,
                    endpoint_url: delivery.url.clone(),
                    event_name: delivery.event_name.clone(),
                    status: "success".to_string(),
                    http_status_code: Some(status_code as i32),
                    response_time_ms: Some(duration.as_millis() as i32),
                    attempt_number: delivery.attempts + 1,
                    error_message: None,
                    filtered_reason: None,
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

                // Log failure to ClickHouse
                let ch_delivery = WebhookDelivery {
                    deployment_id,
                    delivery_id,
                    app_id: delivery.app_id,
                    app_name: delivery.app_name.clone(),
                    endpoint_id: delivery.endpoint_id,
                    endpoint_url: delivery.url.clone(),
                    event_name: delivery.event_name.clone(),
                    status: "failed".to_string(),
                    http_status_code: Some(status_code as i32),
                    response_time_ms: Some(duration.as_millis() as i32),
                    attempt_number: delivery.attempts + 1,
                    error_message: response_body.clone(),
                    filtered_reason: None,
                    timestamp: Utc::now(),
                };

                if let Err(e) = app_state
                    .clickhouse_service
                    .insert_webhook_delivery(&ch_delivery)
                    .await
                {
                    warn!("Failed to log failed delivery to ClickHouse: {}", e);
                }

                handle_delivery_failure(
                    delivery_id,
                    deployment_id,
                    delivery.app_id,
                    delivery.app_name.clone(),
                    delivery.endpoint_id,
                    delivery.url.clone(),
                    delivery.event_name.clone(),
                    delivery.attempts + 1,
                    delivery.max_attempts,
                    Some(status_code),
                    response_body,
                    app_state,
                )
                .await?;

                Ok(DeliveryResult::Failed)
            }
        }
        Err(e) => {
            error!("Failed to deliver webhook {}: {}", delivery_id, e);

            // Log error to ClickHouse
            let ch_delivery = WebhookDelivery {
                deployment_id,
                delivery_id,
                app_id: delivery.app_id,
                app_name: delivery.app_name.clone(),
                endpoint_id: delivery.endpoint_id,
                endpoint_url: delivery.url.clone(),
                event_name: delivery.event_name.clone(),
                status: "failed".to_string(),
                http_status_code: None,
                response_time_ms: None,
                attempt_number: delivery.attempts + 1,
                error_message: Some(e.to_string()),
                filtered_reason: None,
                timestamp: Utc::now(),
            };

            if let Err(e) = app_state
                .clickhouse_service
                .insert_webhook_delivery(&ch_delivery)
                .await
            {
                warn!("Failed to log error delivery to ClickHouse: {}", e);
            }

            handle_delivery_failure(
                delivery_id,
                deployment_id,
                delivery.app_id,
                delivery.app_name,
                delivery.endpoint_id,
                delivery.url,
                delivery.event_name,
                delivery.attempts + 1,
                delivery.max_attempts,
                None,
                Some(e.to_string()),
                app_state,
            )
            .await?;

            Ok(DeliveryResult::Failed)
        }
    }
}

async fn handle_delivery_failure(
    delivery_id: i64,
    deployment_id: i64,
    app_id: i64,
    app_name: String,
    endpoint_id: i64,
    endpoint_url: String,
    event_name: String,
    new_attempts: i32,
    max_attempts: i32,
    status_code: Option<u16>,
    error_message: Option<String>,
    app_state: &AppState,
) -> Result<()> {
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

        info!(
            "Scheduling retry for delivery {} at {}",
            delivery_id, next_retry
        );

        // Update attempts using command
        UpdateDeliveryAttemptsCommand {
            delivery_id,
            new_attempts,
            next_retry_at: next_retry,
        }
        .execute(app_state)
        .await?;

        // Re-queue for later processing
        let retry_delay = (next_retry - Utc::now()).num_seconds().max(1) as u64;
        tokio::time::sleep(tokio::time::Duration::from_secs(retry_delay)).await;

        // Publish retry message to NATS
        let task_message = serde_json::json!({
            "task_type": "webhook.deliver",
            "task_id": format!("webhook-{}-retry-{}", delivery_id, new_attempts),
            "payload": {
                "delivery_id": delivery_id,
                "deployment_id": deployment_id
            }
        });

        app_state
            .nats_client
            .publish(
                "worker.tasks.webhook.deliver",
                serde_json::to_vec(&task_message)?.into(),
            )
            .await?;
    } else {
        warn!(
            "Max attempts reached for delivery {} or non-retryable error",
            delivery_id
        );

        // Get delivery details for archiving using command
        let s3_key = GetFailedDeliveryDetailsCommand { delivery_id }
            .execute(app_state)
            .await?;

        if let Some(s3_key) = s3_key {
            // Retrieve and store failed delivery
            let payload = RetrieveWebhookPayloadCommand::new(s3_key)
                .execute(app_state)
                .await?;

            let error = error_message
                .as_ref()
                .map(|s| s.clone())
                .unwrap_or_else(|| {
                    status_code
                        .map(|s| format!("HTTP {}", s))
                        .unwrap_or_else(|| "Unknown error".to_string())
                });

            StoreFailedWebhookDeliveryCommand::new(delivery_id, payload, error)
                .execute(app_state)
                .await?;
        }

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
                    app_id,
                    app_name: app_name.clone(),
                    endpoint_id,
                    endpoint_url: endpoint_url.clone(),
                    event_name: "endpoint.deactivated".to_string(),
                    status: "deactivated".to_string(),
                    http_status_code: None,
                    response_time_ms: None,
                    attempt_number: 0,
                    error_message: Some(format!(
                        "Auto-deactivated after {} max-attempt failures",
                        DEACTIVATION_THRESHOLD
                    )),
                    filtered_reason: None,
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

        // Log final failure to ClickHouse
        let ch_delivery = WebhookDelivery {
            deployment_id,
            delivery_id,
            app_id,
            app_name,
            endpoint_id,
            endpoint_url,
            event_name,
            status: "permanently_failed".to_string(),
            http_status_code: status_code.map(|s| s as i32),
            response_time_ms: None,
            attempt_number: new_attempts,
            error_message,
            filtered_reason: None,
            timestamp: Utc::now(),
        };

        if let Err(e) = app_state
            .clickhouse_service
            .insert_webhook_delivery(&ch_delivery)
            .await
        {
            warn!("Failed to log permanent failure to ClickHouse: {}", e);
        }
    }

    Ok(())
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
    let mut blocked = 0;
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
                    DeliveryResult::Blocked => blocked += 1,
                    DeliveryResult::NotFound => not_found += 1,
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
        "Batch processed: {} successful, {} failed, {} blocked, {} not found",
        successful, failed, blocked, not_found
    ))
}
