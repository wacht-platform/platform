use chrono::{Datelike, Utc};
use commands::webhook_trigger::TriggerWebhookEventCommand;
use common::{db_router::ReadConsistency, state::AppState};
use queries::{
    b2b::{GetOrganizationDetailsQuery, GetWorkspaceDetailsQuery},
    signin::GetSessionWithSignInsQuery,
    user::{GetUserAuthenticatorQuery, GetUserDetailsQuery},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::info;

use crate::consumer::TaskError;

#[derive(Debug, Deserialize, Serialize)]
pub struct WebhookEventTask {
    pub deployment_id: i64,
    pub event_type: String,
    pub event_payload: serde_json::Value,
    pub triggered_at: chrono::DateTime<Utc>,
}

pub async fn trigger_webhook_event(
    task: WebhookEventTask,
    app_state: &AppState,
) -> Result<String, TaskError> {
    info!(
        "Processing webhook event '{}' for deployment {}",
        task.event_type, task.deployment_id
    );

    let enriched_payload =
        enrich_webhook_payload(task.event_payload.clone(), task.deployment_id, app_state).await?;

    let console_deployment_id = std::env::var("CONSOLE_DEPLOYMENT_ID")
        .map_err(|_| TaskError::Permanent("CONSOLE_DEPLOYMENT_ID not set".to_string()))?
        .parse::<i64>()
        .map_err(|_| TaskError::Permanent("Invalid CONSOLE_DEPLOYMENT_ID".to_string()))?;

    let trigger_command = TriggerWebhookEventCommand::new(
        console_deployment_id,
        task.deployment_id.to_string(),
        task.event_type.clone(),
        enriched_payload,
    );

    trigger_command
        .execute_with(
            app_state.db_router.writer(),
            &app_state.redis_client,
            &app_state.clickhouse_service,
            &app_state.nats_client,
            || Ok(app_state.sf.next_id()? as i64),
        )
        .await
        .map_err(|e| TaskError::Permanent(format!("Failed to trigger webhook event: {}", e)))?;

    track_webhook_billing(task.deployment_id, &app_state.redis_client).await;

    Ok(format!(
        "Webhook event '{}' triggered for deployment {}",
        task.event_type, task.deployment_id
    ))
}

async fn enrich_webhook_payload(
    payload: Value,
    deployment_id: i64,
    app_state: &AppState,
) -> Result<Value, TaskError> {
    let entity_id = payload.get("entity_id").and_then(|v| v.as_i64());
    let entity_type = payload.get("entity_type").and_then(|v| v.as_str());

    let (Some(entity_id), Some(entity_type)) = (entity_id, entity_type) else {
        return Ok(payload);
    };

    info!("Enriching payload for {}:{}", entity_type, entity_id);

    match entity_type {
        "user" => enrich_user_payload(entity_id, payload, deployment_id, app_state).await,
        "organization" => {
            enrich_organization_payload(entity_id, payload, deployment_id, app_state).await
        }
        "workspace" => enrich_workspace_payload(entity_id, payload, deployment_id, app_state).await,
        "session" => enrich_session_payload(entity_id, payload, app_state).await,
        "user_authenticator" => enrich_authenticator_payload(entity_id, payload, app_state).await,
        "user_email" | "user_phone" => {
            enrich_user_payload(entity_id, payload, deployment_id, app_state).await
        }
        _ => Ok(payload),
    }
}

async fn enrich_user_payload(
    user_id: i64,
    mut payload: Value,
    deployment_id: i64,
    app_state: &AppState,
) -> Result<Value, TaskError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let user_details = GetUserDetailsQuery::new(deployment_id, user_id)
        .execute_with(reader)
        .await
        .map_err(|e| TaskError::Permanent(format!("Failed to load user {}: {}", user_id, e)))?;

    let user_json = serde_json::to_value(&user_details)
        .map_err(|e| TaskError::Permanent(format!("Failed to serialize user: {}", e)))?;

    if let Value::Object(ref mut map) = payload {
        if let Value::Object(user_map) = user_json {
            map.extend(user_map);
        }
    }

    Ok(payload)
}

async fn enrich_organization_payload(
    org_id: i64,
    mut payload: Value,
    deployment_id: i64,
    app_state: &AppState,
) -> Result<Value, TaskError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let org_details = GetOrganizationDetailsQuery::new(deployment_id, org_id)
        .execute_with(reader)
        .await
        .map_err(|e| {
            TaskError::Permanent(format!("Failed to load organization {}: {}", org_id, e))
        })?;

    let org_json = serde_json::to_value(&org_details)
        .map_err(|e| TaskError::Permanent(format!("Failed to serialize organization: {}", e)))?;

    if let Value::Object(ref mut map) = payload {
        if let Value::Object(org_map) = org_json {
            map.extend(org_map);
        }
    }

    Ok(payload)
}

async fn enrich_workspace_payload(
    workspace_id: i64,
    mut payload: Value,
    deployment_id: i64,
    app_state: &AppState,
) -> Result<Value, TaskError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let workspace_details = GetWorkspaceDetailsQuery::new(deployment_id, workspace_id)
        .execute_with(reader)
        .await
        .map_err(|e| {
            TaskError::Permanent(format!("Failed to load workspace {}: {}", workspace_id, e))
        })?;

    let workspace_json = serde_json::to_value(&workspace_details)
        .map_err(|e| TaskError::Permanent(format!("Failed to serialize workspace: {}", e)))?;

    if let Value::Object(ref mut map) = payload {
        if let Value::Object(workspace_map) = workspace_json {
            map.extend(workspace_map);
        }
    }

    Ok(payload)
}

async fn enrich_session_payload(
    session_id: i64,
    mut payload: Value,
    app_state: &AppState,
) -> Result<Value, TaskError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let session_data = GetSessionWithSignInsQuery::new(session_id)
        .execute_with(reader)
        .await
        .map_err(|e| {
            TaskError::Permanent(format!("Failed to load session {}: {}", session_id, e))
        })?;

    let session_json = serde_json::to_value(&session_data)
        .map_err(|e| TaskError::Permanent(format!("Failed to serialize session: {}", e)))?;

    if let Value::Object(ref mut map) = payload {
        if let Value::Object(session_map) = session_json {
            map.extend(session_map);
        }
    }

    Ok(payload)
}

async fn enrich_authenticator_payload(
    user_id: i64,
    mut payload: Value,
    app_state: &AppState,
) -> Result<Value, TaskError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let authenticator = GetUserAuthenticatorQuery::new(user_id)
        .execute_with(reader)
        .await
        .map_err(|e| {
            TaskError::Permanent(format!(
                "Failed to load authenticator for user {}: {}",
                user_id, e
            ))
        })?;

    let auth_json = serde_json::to_value(&authenticator)
        .map_err(|e| TaskError::Permanent(format!("Failed to serialize authenticator: {}", e)))?;

    if let Value::Object(ref mut map) = payload {
        if let Value::Object(auth_map) = auth_json {
            map.extend(auth_map);
        }
    }

    Ok(payload)
}

async fn track_webhook_billing(deployment_id: i64, redis_client: &redis::Client) {
    if let Ok(mut conn) = redis_client.get_multiplexed_async_connection().await {
        let now = Utc::now();
        let period = format!("{}-{:02}", now.year(), now.month());
        let prefix = format!("billing:{}:deployment:{}", period, deployment_id);

        let mut pipe = redis::pipe();
        pipe.atomic()
            .zincr(&format!("{}:metrics", prefix), "webhooks", 1)
            .ignore()
            .expire(&format!("{}:metrics", prefix), 5184000)
            .ignore()
            .zincr(
                &format!("billing:{}:dirty_deployments", period),
                deployment_id,
                1,
            )
            .ignore()
            .expire(&format!("{}:metrics", prefix), 5184000)
            .ignore();

        let _: Result<(), redis::RedisError> = pipe.query_async(&mut conn).await;
    }
}
