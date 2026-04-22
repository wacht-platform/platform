use anyhow::Result;
use common::state::AppState;
use dto::clickhouse::ApiKeyVerificationEvent;
use tracing::info;

pub async fn store_api_audit_event_impl(
    task: ApiKeyVerificationEvent,
    app_state: &AppState,
) -> Result<String> {
    app_state
        .clickhouse_service
        .insert_api_audit_log(&task)
        .await?;

    info!(
        "[API AUDIT WORKER] Stored audit event request_id={} deployment_id={} app_slug={}",
        task.request_id, task.deployment_id, task.app_slug
    );

    Ok(format!(
        "API audit event stored successfully for request {}",
        task.request_id
    ))
}
