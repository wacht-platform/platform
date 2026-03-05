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

fn add_direct_recipients(
    recipients: &mut BTreeSet<i64>,
    user_id: Option<FlexibleI64>,
    user_ids: Option<Vec<FlexibleI64>>,
) {
    if let Some(uid) = user_id {
        recipients.insert(uid.0);
    }

    if let Some(user_ids) = user_ids {
        for uid in user_ids {
            recipients.insert(uid.0);
        }
    }
}

async fn add_organization_recipients(
    state: &AppState,
    deployment_id: i64,
    organization_id: Option<i64>,
    recipients: &mut BTreeSet<i64>,
) -> ApiResult<()> {
    let Some(org_id) = organization_id else {
        return Ok(().into());
    };

    let user_ids = GetOrganizationNotificationRecipientUserIdsQuery::new(deployment_id, org_id)
        .execute(state)
        .await?;
    recipients.extend(user_ids);

    Ok(().into())
}

async fn add_workspace_recipients(
    state: &AppState,
    deployment_id: i64,
    workspace_id: Option<i64>,
    recipients: &mut BTreeSet<i64>,
) -> ApiResult<()> {
    let Some(ws_id) = workspace_id else {
        return Ok(().into());
    };

    let user_ids = GetWorkspaceNotificationRecipientUserIdsQuery::new(deployment_id, ws_id)
        .execute(state)
        .await?;
    recipients.extend(user_ids);

    Ok(().into())
}

fn resolve_notification_ctas(
    ctas: Option<JsonValue>,
    action_url: Option<String>,
    action_label: Option<String>,
) -> Option<JsonValue> {
    if let Some(ctas) = ctas {
        return Some(ctas);
    }

    action_url.map(|url| {
        let label = action_label.unwrap_or_else(|| "View".to_string());
        serde_json::json!([{
            "label": label,
            "payload": url
        }])
    })
}

fn apply_optional_notification_fields(
    mut command: CreateNotificationCommand,
    organization_id: Option<i64>,
    workspace_id: Option<i64>,
    ctas: &Option<JsonValue>,
    metadata: &Option<JsonValue>,
    expires_hours: Option<i64>,
) -> CreateNotificationCommand {
    if let Some(org_id) = organization_id {
        command = command.with_organization(org_id);
    }

    if let Some(ws_id) = workspace_id {
        command = command.with_workspace(ws_id);
    }

    if let Some(ctas) = ctas.clone() {
        command = command.with_ctas(ctas);
    }

    if let Some(metadata) = metadata.clone() {
        command = command.with_metadata(metadata);
    }

    if let Some(hours) = expires_hours {
        command = command.with_expiry_hours(hours);
    }

    command
}

/// Create notifications for resolved recipients.
/// Recipients can be provided directly via user_id/user_ids and/or inferred from
/// organization_id/workspace_id membership fanout.
pub async fn create_notification(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateNotificationRequest>,
) -> ApiResult<Vec<Notification>> {
    let CreateNotificationRequest {
        user_id,
        user_ids,
        organization_id,
        workspace_id,
        title,
        body,
        action_url,
        action_label,
        ctas,
        severity,
        metadata,
        expires_hours,
    } = request;

    let mut recipients = BTreeSet::<i64>::new();
    add_direct_recipients(&mut recipients, user_id, user_ids);

    let organization_id = organization_id.map(|value| value.0);
    let workspace_id = workspace_id.map(|value| value.0);
    add_organization_recipients(&state, deployment_id, organization_id, &mut recipients).await?;
    add_workspace_recipients(&state, deployment_id, workspace_id, &mut recipients).await?;

    if recipients.is_empty() {
        return Err(bad_request(
            "At least one recipient must be specified via user_id, user_ids, organization_id, or workspace_id",
        )
        .into());
    }

    let ctas_to_apply = resolve_notification_ctas(ctas, action_url, action_label);

    let severity = severity
        .as_deref()
        .map(NotificationSeverity::from)
        .unwrap_or(NotificationSeverity::Info);

    let mut created = Vec::with_capacity(recipients.len());
    for uid in recipients {
        let command = CreateNotificationCommand::new(deployment_id, title.clone(), body.clone())
        .with_user(uid)
        .with_severity(severity.clone());
        let command = apply_optional_notification_fields(
            command,
            organization_id,
            workspace_id,
            &ctas_to_apply,
            &metadata,
            expires_hours,
        );

        created.push(command.execute(&state).await?);
    }

    Ok(created.into())
}
