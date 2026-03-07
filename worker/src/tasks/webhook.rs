use anyhow::Result;
use chrono::{SecondsFormat, Utc};
use commands::{
    SendEmailCommand,
    webhook_delivery::{
        ClearEndpointFailuresCommand, DeactivateEndpointCommand, DeleteActiveDeliveryCommand,
        GetActiveDeliveryCommand, IncrementEndpointFailuresCommand, UpdateDeliveryAttemptsCommand,
        calculate_next_retry,
    },
    webhook_subscription::evaluate_filter,
};
use common::db_router::ReadConsistency;
use common::state::AppState;
use common::utils::webhook::generate_webhook_signature;
use dto::clickhouse::webhook::WebhookLog;
use queries::GetWebhookAppByNameQuery;
use reqwest::header::RETRY_AFTER;
use serde::{Deserialize, Serialize};
use serde_json::json;
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
    RetryAfter(std::time::Duration),
}

const STATUS_REQUEST_TIMEOUT: u16 = 408;
const STATUS_TOO_MANY_REQUESTS: u16 = 429;
const STATUS_INTERNAL_SERVER_ERROR: u16 = 500;
const WEBHOOK_FAILURE_EMAIL_COOLDOWN_SECONDS: u64 = 3600;

fn parse_retry_after_header(
    value: Option<&reqwest::header::HeaderValue>,
) -> Option<std::time::Duration> {
    let raw = value?.to_str().ok()?.trim();
    if raw.is_empty() {
        return None;
    }

    if let Ok(seconds) = raw.parse::<u64>() {
        return Some(std::time::Duration::from_secs(seconds.max(1)));
    }

    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(raw) {
        let now = Utc::now();
        let target = dt.with_timezone(&Utc);
        let delta_seconds = (target - now).num_seconds();
        if delta_seconds > 0 {
            return Some(std::time::Duration::from_secs(delta_seconds as u64));
        }
    }

    None
}

pub async fn process_webhook_delivery(
    delivery_id: i64,
    deployment_id: i64,
    app_state: &AppState,
) -> Result<DeliveryResult> {
    let command = GetActiveDeliveryCommand { delivery_id };
    let delivery = match command.execute_with_db(app_state.db_router.writer()).await? {
        Some(d) => d,
        None => {
            warn!("Webhook delivery {} not found", delivery_id);
            return Ok(DeliveryResult::NotFound);
        }
    };

    let payload = match delivery.payload {
        Some(payload_value) => payload_value,
        None => {
            warn!("Webhook delivery {} has no payload", delivery_id);
            return Ok(DeliveryResult::NotFound);
        }
    };

    let event_timestamp = chrono::DateTime::<Utc>::from_timestamp(delivery.webhook_timestamp, 0)
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let outbound_payload = json!({
        "type": delivery.event_name,
        "timestamp": event_timestamp,
        "data": payload,
    });

    if let Some(filter_rules) = &delivery.filter_rules {
        let filter_match = evaluate_filter(filter_rules, &payload);
        info!(
            delivery_id,
            deployment_id,
            endpoint_id = delivery.endpoint_id,
            event_name = %delivery.event_name,
            filter_match,
            "Worker evaluated snapshot filter rules for delivery",
        );

        if !filter_match {
            let payload_json = serde_json::to_string(&outbound_payload).unwrap_or_default();
            let ch_delivery = WebhookLog {
                deployment_id,
                delivery_id,
                app_slug: delivery.app_slug.clone(),
                endpoint_id: delivery.endpoint_id,
                event_name: delivery.event_name.clone(),
                status: "filtered".to_string(),
                http_status_code: None,
                response_time_ms: None,
                attempt_number: delivery.attempts,
                max_attempts: delivery.max_attempts,
                payload: Some(payload_json.clone()),
                payload_size_bytes: payload_json.len() as i32,
                response_body: None,
                response_headers: None,
                request_headers: None,
                timestamp: Utc::now(),
            };

            if let Err(e) = app_state
                .clickhouse_service
                .insert_webhook_log(&ch_delivery)
                .await
            {
                warn!("Failed to log filtered delivery to Tinybird: {}", e);
            }

            DeleteActiveDeliveryCommand { delivery_id }
                .execute_with_db(app_state.db_router.writer())
                .await?;
            return Ok(DeliveryResult::Success);
        }
    }

    info!(
        delivery_id,
        deployment_id,
        endpoint_id = delivery.endpoint_id,
        app_slug = %delivery.app_slug,
        event_name = %delivery.event_name,
        "Processing queued webhook delivery",
    );

    if let Some(rate_limit_config) = &delivery.rate_limit_config {
        let config: models::webhook::RateLimitConfig =
            serde_json::from_value(rate_limit_config.clone())
                .map_err(|e| anyhow::anyhow!("Invalid rate limit config: {}", e))?;

        let throttler = crate::throttler::WebhookThrottler::new(app_state.redis_client.clone());

        match throttler
            .check_and_record(
                delivery.endpoint_id,
                config.duration_ms,
                config.max_requests,
            )
            .await?
        {
            None => {
                info!(
                    "Webhook delivery {} allowed (limit: {} per {}ms)",
                    delivery_id, config.max_requests, config.duration_ms
                );
            }
            Some(delay_ms) => {
                info!(
                    "Webhook delivery {} rate limited, requeuing with {}ms delay (limit: {} per {}ms)",
                    delivery_id, delay_ms, config.max_requests, config.duration_ms
                );
                return Ok(DeliveryResult::RetryAfter(
                    std::time::Duration::from_millis(delay_ms as u64),
                ));
            }
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            delivery.timeout_seconds as u64,
        ))
        .user_agent("Wacht-Webhook/1.0")
        .build()?;

    let payload_json = serde_json::to_string(&outbound_payload).unwrap_or_default();

    let mut request = client.post(&delivery.url).json(&outbound_payload);

    let mut final_headers = std::collections::HashMap::new();
    final_headers.insert("webhook-id".to_string(), delivery.webhook_id.clone());
    final_headers.insert(
        "webhook-timestamp".to_string(),
        delivery.webhook_timestamp.to_string(),
    );
    let signature = generate_webhook_signature(
        &delivery.signing_secret,
        &delivery.webhook_id,
        delivery.webhook_timestamp,
        &outbound_payload,
    );
    final_headers.insert("webhook-signature".to_string(), signature);

    if let Some(headers) = &delivery.headers {
        if let Some(headers_obj) = headers.as_object() {
            for (key, value) in headers_obj {
                if let Some(value_str) = value.as_str() {
                    final_headers.insert(key.clone(), value_str.to_string());
                }
            }
        }
    }

    let request_headers_json = serde_json::to_string(&final_headers).ok();

    for (k, v) in &final_headers {
        request = request.header(k, v);
    }

    let start = Instant::now();
    let result = request.send().await;
    let duration = start.elapsed();

    match result {
        Ok(response) => {
            let status = response.status();
            let status_code = status.as_u16();
            let retry_after = parse_retry_after_header(response.headers().get(RETRY_AFTER));
            let response_body = response.text().await.ok();

            if status_code == 410 {
                warn!(
                    "Endpoint {} returned 410 Gone, permanently disabling endpoint",
                    delivery.url
                );

                let ch_delivery = WebhookLog {
                    deployment_id,
                    delivery_id,
                    app_slug: delivery.app_slug.clone(),
                    endpoint_id: delivery.endpoint_id,
                    event_name: delivery.event_name.clone(),
                    status: "endpoint_disabled".to_string(),
                    http_status_code: Some(410),
                    response_time_ms: Some(duration.as_millis() as i32),
                    attempt_number: delivery.attempts + 1,
                    max_attempts: delivery.max_attempts,
                    payload: Some(payload_json.clone()),
                    payload_size_bytes: payload_json.len() as i32,
                    response_body: response_body.clone(),
                    response_headers: None,
                    request_headers: request_headers_json.clone(),
                    timestamp: Utc::now(),
                };

                if let Err(e) = app_state
                    .clickhouse_service
                    .insert_webhook_log(&ch_delivery)
                    .await
                {
                    warn!("Failed to log 410 Gone delivery to ClickHouse: {}", e);
                }

                DeleteActiveDeliveryCommand { delivery_id }
                    .execute_with_db(app_state.db_router.writer())
                    .await?;

                DeactivateEndpointCommand {
                    endpoint_id: delivery.endpoint_id,
                }
                .execute_with_db(app_state.db_router.writer())
                .await?;

                info!(
                    "Endpoint {} (ID: {}) has been permanently disabled due to 410 Gone response",
                    delivery.url, delivery.endpoint_id
                );

                if let Err(e) = send_webhook_failure_notification(
                    deployment_id,
                    &delivery.app_slug,
                    delivery.endpoint_id,
                    &delivery.url,
                    app_state,
                )
                .await
                {
                    warn!(
                        "Failed to send webhook endpoint failure notification for endpoint {}: {}",
                        delivery.endpoint_id, e
                    );
                }

                return Ok(DeliveryResult::Failed);
            }

            if status.is_success() {
                let ch_delivery = WebhookLog {
                    deployment_id,
                    delivery_id,
                    app_slug: delivery.app_slug.clone(),
                    endpoint_id: delivery.endpoint_id,
                    event_name: delivery.event_name.clone(),
                    status: "success".to_string(),
                    http_status_code: Some(status_code as i32),
                    response_time_ms: Some(duration.as_millis() as i32),
                    attempt_number: delivery.attempts + 1,
                    max_attempts: delivery.max_attempts,
                    payload: Some(payload_json.clone()),
                    payload_size_bytes: payload_json.len() as i32,
                    response_body: response_body.clone(),
                    response_headers: None,
                    request_headers: request_headers_json.clone(),
                    timestamp: Utc::now(),
                };

                if let Err(e) = app_state
                    .clickhouse_service
                    .insert_webhook_log(&ch_delivery)
                    .await
                {
                    warn!("Failed to log successful delivery to ClickHouse: {}", e);
                }

                DeleteActiveDeliveryCommand { delivery_id }
                    .execute_with_db(app_state.db_router.writer())
                    .await?;

                Ok(DeliveryResult::Success)
            } else {
                warn!(
                    "Webhook {} returned non-success status: {}",
                    delivery_id, status
                );

                let will_retry = (delivery.attempts + 1) < delivery.max_attempts
                    && (status_code >= 500 || status_code == 408 || status_code == 429);

                let ch_delivery = WebhookLog {
                    deployment_id,
                    delivery_id,
                    app_slug: delivery.app_slug.clone(),
                    endpoint_id: delivery.endpoint_id,
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
                    payload: Some(payload_json.clone()),
                    payload_size_bytes: payload_json.len() as i32,
                    response_body: response_body.clone(),
                    response_headers: None,
                    request_headers: request_headers_json.clone(),
                    timestamp: Utc::now(),
                };

                if let Err(e) = app_state
                    .clickhouse_service
                    .insert_webhook_log(&ch_delivery)
                    .await
                {
                    warn!("Failed to log failed delivery to ClickHouse: {}", e);
                }

                let result = handle_delivery_failure(
                    delivery_id,
                    deployment_id,
                    delivery.app_slug.clone(),
                    delivery.endpoint_id,
                    delivery.url.clone(),
                    delivery.attempts + 1,
                    delivery.max_attempts,
                    Some(status_code),
                    retry_after,
                    app_state,
                )
                .await?;

                Ok(result)
            }
        }
        Err(_e) => {
            let will_retry = (delivery.attempts + 1) < delivery.max_attempts;
            let ch_delivery = WebhookLog {
                deployment_id,
                delivery_id,
                app_slug: delivery.app_slug.clone(),
                endpoint_id: delivery.endpoint_id,
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
                payload: Some(payload_json.clone()),
                payload_size_bytes: payload_json.len() as i32,
                response_body: None,
                response_headers: None,
                request_headers: request_headers_json.clone(),
                timestamp: Utc::now(),
            };

            if let Err(e) = app_state
                .clickhouse_service
                .insert_webhook_log(&ch_delivery)
                .await
            {
                warn!("Failed to log error delivery to ClickHouse: {}", e);
            }

            let result = handle_delivery_failure(
                delivery_id,
                deployment_id,
                delivery.app_slug,
                delivery.endpoint_id,
                delivery.url,
                delivery.attempts + 1,
                delivery.max_attempts,
                None,
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
    app_slug: String,
    endpoint_id: i64,
    endpoint_url: String,
    new_attempts: i32,
    max_attempts: i32,
    status_code: Option<u16>,
    retry_after: Option<std::time::Duration>,
    app_state: &AppState,
) -> Result<DeliveryResult> {
    let should_retry = new_attempts < max_attempts
        && status_code.map_or(true, |s| {
            s >= STATUS_INTERNAL_SERVER_ERROR
                || s == STATUS_REQUEST_TIMEOUT
                || s == STATUS_TOO_MANY_REQUESTS
        });

    if should_retry {
        let retry_delay = if let Some(retry_after_duration) = retry_after {
            retry_after_duration
        } else {
            let next_retry = calculate_next_retry(new_attempts);
            std::time::Duration::from_secs((next_retry - Utc::now()).num_seconds().max(1) as u64)
        };
        let next_retry = Utc::now()
            + chrono::Duration::from_std(retry_delay)
                .unwrap_or_else(|_| chrono::Duration::hours(6));

        info!(
            "Scheduling retry for delivery {} at {} (delay: {}s)",
            delivery_id,
            next_retry,
            retry_delay.as_secs()
        );

        UpdateDeliveryAttemptsCommand {
            delivery_id,
            new_attempts,
            next_retry_at: next_retry,
        }
        .execute_with_db(app_state.db_router.writer())
        .await?;

        return Ok(DeliveryResult::RetryAfter(std::time::Duration::from_secs(
            retry_delay.as_secs(),
        )));
    } else {
        warn!(
            "Max attempts reached for delivery {} or non-retryable error",
            delivery_id
        );

        DeleteActiveDeliveryCommand { delivery_id }
            .execute_with_db(app_state.db_router.writer())
            .await?;

        if new_attempts >= max_attempts {
            let failure_count = IncrementEndpointFailuresCommand { endpoint_id }
                .execute_with_deps(&app_state.redis_client)
                .await?;

            const DEACTIVATION_THRESHOLD: i64 = 10;
            if failure_count >= DEACTIVATION_THRESHOLD {
                warn!(
                    "Auto-deactivating endpoint {} after {} max-attempt failures in 24 hours",
                    endpoint_id, failure_count
                );

                DeactivateEndpointCommand { endpoint_id }
                    .execute_with_db(app_state.db_router.writer())
                    .await?;

                ClearEndpointFailuresCommand { endpoint_id }
                    .execute_with_deps(&app_state.redis_client)
                    .await?;

                if let Ok(log_id) = app_state.sf.next_id() {
                    let ch_delivery = WebhookLog {
                        deployment_id,
                        delivery_id: log_id as i64,
                        app_slug: app_slug.clone(),
                        endpoint_id,
                        event_name: "endpoint.deactivated".to_string(),
                        status: "deactivated".to_string(),
                        http_status_code: None,
                        response_time_ms: None,
                        attempt_number: 0,
                        max_attempts: 0,
                        payload: None,
                        payload_size_bytes: 0,
                        response_body: None,
                        response_headers: None,
                        request_headers: None,
                        timestamp: Utc::now(),
                    };

                    if let Err(e) = app_state
                        .clickhouse_service
                        .insert_webhook_log(&ch_delivery)
                        .await
                    {
                        warn!("Failed to log endpoint deactivation to ClickHouse: {}", e);
                    }
                } else {
                    warn!("Failed to generate snowflake id for endpoint deactivation log");
                }

                info!(
                    "Endpoint {} for app {} has been auto-deactivated. Customer should be notified.",
                    endpoint_id, app_slug
                );

                if let Err(e) = send_webhook_failure_notification(
                    deployment_id,
                    &app_slug,
                    endpoint_id,
                    &endpoint_url,
                    app_state,
                )
                .await
                {
                    warn!(
                        "Failed to send webhook endpoint failure notification for endpoint {}: {}",
                        endpoint_id, e
                    );
                }
            }
        }
    }

    Ok(DeliveryResult::Failed)
}

async fn get_failure_notification_emails(
    deployment_id: i64,
    app_slug: &str,
    app_state: &AppState,
) -> Result<Vec<String>> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let app = GetWebhookAppByNameQuery::new(deployment_id, app_slug.to_string())
        .execute_with_db(reader)
        .await?;

    let Some(app) = app else {
        return Ok(Vec::new());
    };

    let emails = app
        .failure_notification_emails
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|email| !email.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    Ok(emails)
}

async fn send_webhook_failure_notification(
    deployment_id: i64,
    app_slug: &str,
    endpoint_id: i64,
    endpoint_url: &str,
    app_state: &AppState,
) -> Result<()> {
    let recipient_emails =
        get_failure_notification_emails(deployment_id, app_slug, app_state).await?;

    if recipient_emails.is_empty() {
        return Ok(());
    }

    let mut redis_conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await?;

    for recipient_email in recipient_emails {
        let dedupe_key = format!(
            "webhook:failure-notification:{}:{}:{}:{}",
            deployment_id,
            app_slug,
            endpoint_id,
            recipient_email.to_lowercase()
        );

        let lock_set: Option<String> = redis::cmd("SET")
            .arg(&dedupe_key)
            .arg("1")
            .arg("NX")
            .arg("EX")
            .arg(WEBHOOK_FAILURE_EMAIL_COOLDOWN_SECONDS)
            .query_async(&mut redis_conn)
            .await?;

        if lock_set.is_none() {
            info!(
                deployment_id,
                app_slug,
                endpoint_id,
                recipient = %recipient_email,
                "Skipping duplicate webhook failure notification within cooldown",
            );
            continue;
        }

        let variables = serde_json::json!({
            "endpoint": {
                "url": endpoint_url,
            },
        });

        let send_email_command = SendEmailCommand::new(
            deployment_id,
            "webhook_failure_notification_template".to_string(),
            recipient_email.clone(),
            variables,
        );
        if let Err(e) = send_email_command.execute_with_deps(app_state).await {
            error!(
                deployment_id,
                app_slug,
                endpoint_id,
                recipient = %recipient_email,
                "Failed to send webhook failure notification email: {}",
                e
            );
        } else {
            info!(
                deployment_id,
                app_slug,
                endpoint_id,
                recipient = %recipient_email,
                "Webhook failure notification email sent",
            );
        }
    }

    Ok(())
}

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

        for handle in handles {
            match handle.await {
                Ok(Ok(result)) => match result {
                    DeliveryResult::Success => successful += 1,
                    DeliveryResult::Failed => failed += 1,
                    DeliveryResult::NotFound => not_found += 1,
                    DeliveryResult::RetryAfter(_) => failed += 1,
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
    use commands::webhook_trigger::{ReplayWebhookDeliveryCommand, ReplayWebhookDeliveryDeps};

    info!(
        "Processing webhook retry for delivery {} in deployment {}",
        delivery_id, deployment_id
    );

    let replay_command = ReplayWebhookDeliveryCommand {
        delivery_id,
        deployment_id,
    };
    let new_delivery_id = replay_command
        .execute_with_deps(ReplayWebhookDeliveryDeps {
            db_router: &app_state.db_router,
            clickhouse_service: &app_state.clickhouse_service,
            nats_client: &app_state.nats_client,
            id_gen: || Ok(app_state.sf.next_id()? as i64),
        })
        .await
        .map_err(|e| anyhow::anyhow!("Failed to replay webhook delivery: {}", e))?;

    Ok(format!(
        "Webhook delivery {} retried as new delivery {}",
        delivery_id, new_delivery_id
    ))
}
