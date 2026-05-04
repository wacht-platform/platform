use anyhow::Result;
use common::state::AppState;
use dto::clickhouse::ApiKeyVerificationEvent;

pub async fn store_api_audit_event_impl(
    task: ApiKeyVerificationEvent,
    app_state: &AppState,
) -> Result<String> {
    app_state
        .clickhouse_service
        .insert_api_audit_log(&task)
        .await?;

    Ok(format!(
        "API audit event stored successfully for request {}",
        task.request_id
    ))
}
