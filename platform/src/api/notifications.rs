use crate::{application::response::ApiResult, middleware::RequireDeployment};
use axum::{Json, extract::State, http::StatusCode};
use commands::{Command, notification::CreateNotificationCommand};
use common::state::AppState;
use dto::json::FlexibleI64;
use models::notification::{Notification, NotificationSeverity};
use queries::{
    GetOrganizationNotificationRecipientUserIdsQuery,
    GetWorkspaceNotificationRecipientUserIdsQuery, Query as QueryTrait,
};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::collections::BTreeSet;

#[derive(Debug, Deserialize)]
pub struct CreateNotificationRequest {
    pub user_id: Option<FlexibleI64>,
    pub user_ids: Option<Vec<FlexibleI64>>,
    pub organization_id: Option<FlexibleI64>,
    pub workspace_id: Option<FlexibleI64>,
    pub title: String,
    pub body: String,
    pub action_url: Option<String>,
    pub action_label: Option<String>,
    pub ctas: Option<JsonValue>,
    pub severity: Option<String>,
    pub metadata: Option<JsonValue>,
    pub expires_hours: Option<i64>,
}

fn bad_request(msg: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, msg.into())
}

/// Create notifications for resolved recipients.
/// Recipients can be provided directly via user_id/user_ids and/or inferred from
/// organization_id/workspace_id membership fanout.
pub async fn create_notification(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateNotificationRequest>,
) -> ApiResult<Vec<Notification>> {
    let mut recipients = BTreeSet::<i64>::new();

    if let Some(uid) = request.user_id {
        recipients.insert(uid.0);
    }

    if let Some(user_ids) = request.user_ids {
        for uid in user_ids {
            recipients.insert(uid.0);
        }
    }

    let organization_id = request.organization_id.map(|v| v.0);
    let workspace_id = request.workspace_id.map(|v| v.0);

    if let Some(org_id) = organization_id {
        let user_ids = GetOrganizationNotificationRecipientUserIdsQuery::new(deployment_id, org_id)
            .execute(&state)
            .await?;
        for user_id in user_ids {
            recipients.insert(user_id);
        }
    }

    if let Some(ws_id) = workspace_id {
        let user_ids = GetWorkspaceNotificationRecipientUserIdsQuery::new(deployment_id, ws_id)
            .execute(&state)
            .await?;
        for user_id in user_ids {
            recipients.insert(user_id);
        }
    }

    if recipients.is_empty() {
        return Err(bad_request(
            "At least one recipient must be specified via user_id, user_ids, organization_id, or workspace_id",
        )
        .into());
    }

    let ctas_to_apply = if let Some(ctas) = request.ctas {
        Some(ctas)
    } else if let Some(url) = request.action_url {
        let label = request.action_label.unwrap_or_else(|| "View".to_string());
        Some(serde_json::json!([{
            "label": label,
            "payload": url
        }]))
    } else {
        None
    };

    let severity = request
        .severity
        .as_deref()
        .map(NotificationSeverity::from)
        .unwrap_or(NotificationSeverity::Info);

    let mut created = Vec::with_capacity(recipients.len());
    for uid in recipients {
        let mut command = CreateNotificationCommand::new(
            deployment_id,
            request.title.clone(),
            request.body.clone(),
        )
        .with_user(uid)
        .with_severity(severity.clone());

        if let Some(org_id) = organization_id {
            command = command.with_organization(org_id);
        }

        if let Some(ws_id) = workspace_id {
            command = command.with_workspace(ws_id);
        }

        if let Some(ctas) = ctas_to_apply.clone() {
            command = command.with_ctas(ctas);
        }

        if let Some(metadata) = request.metadata.clone() {
            command = command.with_metadata(metadata);
        }

        if let Some(hours) = request.expires_hours {
            command = command.with_expiry_hours(hours);
        }

        created.push(command.execute(&state).await?);
    }

    Ok(created.into())
}
