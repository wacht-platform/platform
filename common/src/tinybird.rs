use super::clickhouse::UserEvent;
use crate::error::AppError;
use dto::clickhouse::ApiKeyVerificationEvent;
use dto::clickhouse::webhook::{WebhookLog, WebhookLogLight};
use reqwest::Client;
use serde::Serialize;

const TINYBIRD_EVENTS_URL: &str = "https://api.us-east.aws.tinybird.co/v0/events";

pub async fn insert_event<T: Serialize>(data_source: &str, event: &T) -> Result<(), AppError> {
    let client = Client::new();
    let token = std::env::var("TINYBIRD_TOKEN")
        .map_err(|_| AppError::Internal("TINYBIRD_TOKEN not set".to_string()))?;

    let url = format!("{}?name={}", TINYBIRD_EVENTS_URL, data_source);
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(event)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Tinybird insert failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "Tinybird insert to {} failed: {} - {}",
            data_source, status, body
        )));
    }

    Ok(())
}

pub async fn insert_events_batch<T: Serialize>(
    data_source: &str,
    events: &[T],
) -> Result<(), AppError> {
    if events.is_empty() {
        return Ok(());
    }

    let client = Client::new();
    let token = std::env::var("TINYBIRD_TOKEN")
        .map_err(|_| AppError::Internal("TINYBIRD_TOKEN not set".to_string()))?;

    let url = format!("{}?name={}", TINYBIRD_EVENTS_URL, data_source);
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(events)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Tinybird batch insert failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "Tinybird batch insert to {} failed: {} - {}",
            data_source, status, body
        )));
    }

    Ok(())
}

pub async fn insert_webhook_log(log: &WebhookLog) -> Result<(), AppError> {
    insert_event("webhook_logs_full", log).await?;

    let log_light = WebhookLogLight {
        deployment_id: log.deployment_id,
        delivery_id: log.delivery_id,
        app_slug: log.app_slug.clone(),
        endpoint_id: log.endpoint_id,
        event_name: log.event_name.clone(),
        status: log.status.clone(),
        http_status_code: log.http_status_code,
        response_time_ms: log.response_time_ms,
        attempt_number: log.attempt_number,
        max_attempts: log.max_attempts,
        payload_size_bytes: log.payload_size_bytes,
        timestamp: log.timestamp,
    };

    insert_event("webhook_logs_light", &log_light).await?;

    Ok(())
}

pub async fn insert_webhook_logs_batch(logs: &[WebhookLog]) -> Result<(), AppError> {
    if logs.is_empty() {
        return Ok(());
    }

    let light_logs: Vec<WebhookLogLight> = logs
        .iter()
        .map(|log| WebhookLogLight {
            deployment_id: log.deployment_id,
            delivery_id: log.delivery_id,
            app_slug: log.app_slug.clone(),
            endpoint_id: log.endpoint_id,
            event_name: log.event_name.clone(),
            status: log.status.clone(),
            http_status_code: log.http_status_code,
            response_time_ms: log.response_time_ms,
            attempt_number: log.attempt_number,
            max_attempts: log.max_attempts,
            payload_size_bytes: log.payload_size_bytes,
            timestamp: log.timestamp,
        })
        .collect();

    insert_events_batch("webhook_logs_full", logs).await?;
    insert_events_batch("webhook_logs_light", &light_logs).await?;

    Ok(())
}

pub async fn insert_user_event(event: &UserEvent) -> Result<(), AppError> {
    insert_event("user_events", event).await
}

pub async fn insert_api_audit_log(event: &ApiKeyVerificationEvent) -> Result<(), AppError> {
    insert_event("api_audit_logs", event).await
}

pub fn insert_api_audit_log_async(event: ApiKeyVerificationEvent) {
    tokio::spawn(async move {
        if let Err(e) = insert_api_audit_log(&event).await {
            tracing::warn!(error = %e, "Failed to insert API audit log to Tinybird");
        }
    });
}
