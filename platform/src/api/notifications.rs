use crate::{
    application::{HttpState, response::ApiResult},
    middleware::RequireDeployment,
};
use axum::{
    extract::State,
    Json,
};
use serde::Deserialize;
use shared::{
    commands::{notification::CreateNotificationCommand, Command},
    models::notification::Notification,
};

#[derive(Debug, Deserialize)]
pub struct CreateNotificationRequest {
    pub user_id: i64,
    pub title: String,
    pub body: String,
    pub action_url: Option<String>,
    pub action_label: Option<String>,
    pub severity: Option<String>,
    pub group_id: Option<String>,
    pub dedupe_key: Option<String>,
    pub source: Option<String>,
    pub source_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub expires_hours: Option<i64>,
}

/// Create a notification for a specific user
pub async fn create_notification(
    State(state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateNotificationRequest>,
) -> ApiResult<Notification> {
    let mut command = CreateNotificationCommand::new(
        deployment_id,
        request.user_id,
        request.title,
        request.body,
    );
    
    if let Some(url) = request.action_url {
        if let Some(label) = request.action_label {
            command = command.with_action(url, label);
        }
    }
    
    if let Some(severity_str) = request.severity {
        use shared::models::notification::NotificationSeverity;
        let severity = NotificationSeverity::from(&severity_str);
        command = command.with_severity(severity);
    }
    
    if let Some(source) = request.source {
        command = command.with_source(source, request.source_id);
    }
    
    if let Some(group_id) = request.group_id {
        command = command.with_group(group_id);
    }
    
    if let Some(dedupe_key) = request.dedupe_key {
        command = command.with_dedupe_key(dedupe_key);
    }
    
    if let Some(metadata) = request.metadata {
        command = command.with_metadata(metadata);
    }
    
    if let Some(hours) = request.expires_hours {
        command = command.with_expiry_hours(hours);
    }
    
    let notification = command.execute(&state).await?;
    
    Ok(notification.into())
}