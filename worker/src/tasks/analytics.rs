use anyhow::Result;
use chrono::{DateTime, Utc};
use common::clickhouse::UserEvent;
use common::state::AppState;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
pub struct AnalyticsEventTask {
    pub deployment_id: u64,
    pub user_id: Option<u64>,
    pub event_type: String,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub auth_method: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub ip_address: Option<String>,
}

pub async fn store_analytics_event_impl(
    task: AnalyticsEventTask,
    app_state: &AppState,
) -> Result<String> {
    info!(
        "[ANALYTICS WORKER] Processing {} event for deployment {} (user: {:?})",
        task.event_type, task.deployment_id, task.user_id
    );

    let user_event = UserEvent {
        deployment_id: task.deployment_id as i64,
        user_id: task.user_id.map(|id| id as i64),
        event_type: task.event_type.clone(),
        user_name: task.user_name,
        user_email: task.user_email,
        auth_method: task.auth_method,
        timestamp: task.timestamp,
        ip_address: task.ip_address,
    };

    app_state
        .clickhouse_service
        .insert_user_event(&user_event)
        .await?;

    info!(
        "[ANALYTICS WORKER] Successfully stored {} event for deployment {}",
        task.event_type, task.deployment_id
    );

    Ok(format!(
        "Analytics event {} stored successfully",
        task.event_type
    ))
}
