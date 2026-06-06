use anyhow::Result;
use chrono::{DateTime, Utc};
use common::clickhouse::UserEvent;
use common::state::AppState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct AnalyticsEventTask {
    pub deployment_id: u64,
    pub user_id: Option<u64>,
    pub event_type: String,
    pub user_name: Option<String>,
    pub user_identifier: Option<String>,
    pub auth_method: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub ip_address: Option<String>,
    pub country: Option<String>,
    pub device: Option<String>,
}

pub async fn store_analytics_event_impl(
    task: AnalyticsEventTask,
    app_state: &AppState,
) -> Result<String> {
    let user_event = UserEvent {
        deployment_id: task.deployment_id as i64,
        user_id: task.user_id.map(|id| id as i64),
        event_type: task.event_type.clone(),
        user_name: task.user_name,
        user_identifier: task.user_identifier,
        auth_method: task.auth_method,
        timestamp: task.timestamp,
        ip_address: task.ip_address,
        country: task.country,
        device: task.device,
    };

    app_state
        .clickhouse_service
        .insert_user_event(&user_event)
        .await?;

    Ok(format!(
        "Analytics event {} stored successfully",
        task.event_type
    ))
}
