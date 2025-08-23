use axum::{
    Extension,
    extract::{Query, State},
    http::{HeaderMap, header::COOKIE},
    response::IntoResponse,
};
use common::utils::jwt::verify_token;
use fastwebsockets::{FragmentCollector, Frame, OpCode, WebSocketError, upgrade};
use futures::StreamExt;
use queries::{
    GetDeploymentWithKeyPairQuery, GetSessionWithActiveContextQuery, Query as QueryTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info, warn};

use crate::application::HttpState;
use crate::middleware::host_extractor::ExtractedHost;

#[derive(Debug, Deserialize)]
pub struct NotificationParams {
    pub host: Option<String>,
    pub channels: Option<Vec<String>>,
    pub organization_ids: Option<Vec<i64>>,
    pub workspace_ids: Option<Vec<i64>>,
    #[serde(rename = "__dev_session__")]
    pub dev_session: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SessionClaims {
    pub sess: String,
    pub rotating_token: String,
    pub exp: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationMessage {
    pub id: i64,
    pub user_id: i64,
    pub deployment_id: i64,
    pub organization_id: Option<i64>,
    pub workspace_id: Option<i64>,
    pub title: String,
    pub body: String,
    pub severity: String,
    pub action_url: Option<String>,
    pub action_label: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn notification_stream_handler(
    Extension(ExtractedHost(host)): Extension<ExtractedHost>,
    headers: HeaderMap,
    Query(params): Query<NotificationParams>,
    ws: upgrade::IncomingUpgrade,
    State(state): State<HttpState>,
) -> impl IntoResponse {
    // Extract session token from cookies or query params (like frontend API)
    let session_token = extract_session_token(&headers, &params);
    if session_token.is_none() {
        warn!("WebSocket connection attempted without session token");
        return axum::response::Response::builder()
            .status(401)
            .body(axum::body::Body::from("Session token required"))
            .unwrap()
            .into_response();
    }

    let token = session_token.unwrap();

    let (response, fut) = ws.upgrade().unwrap();

    tokio::task::spawn(async move {
        if let Err(e) = handle_notification_client(fut, state, host, token, params).await {
            error!("Error in notification websocket connection: {e}");
        }
    });

    response.into_response()
}

fn extract_session_token(headers: &HeaderMap, params: &NotificationParams) -> Option<String> {
    // First try cookies (production mode)
    if let Some(cookie_header) = headers.get(COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            // Parse cookies to find __session
            for cookie in cookie_str.split(';') {
                let cookie = cookie.trim();
                if let Some((name, value)) = cookie.split_once('=') {
                    if name == "__session" {
                        return Some(value.to_string());
                    }
                }
            }
        }
    }

    // Fallback to query params (development mode)
    params.dev_session.clone()
}

async fn handle_notification_client(
    fut: upgrade::UpgradeFut,
    app_state: HttpState,
    host: String,
    token: String,
    params: NotificationParams,
) -> Result<(), WebSocketError> {
    let mut ws = FragmentCollector::new(fut.await?);

    // Get deployment ID and public key from host
    let (deployment_id, public_key) = match GetDeploymentWithKeyPairQuery::new(host.clone())
        .execute(&app_state)
        .await
    {
        Ok(result) => result,
        Err(e) => {
            error!("Failed to get deployment for host {}: {}", host, e);
            let error_msg = json!({
                "error": "Invalid deployment"
            });
            let _ = ws
                .write_frame(Frame::text(fastwebsockets::Payload::Owned(
                    serde_json::to_vec(&error_msg).unwrap(),
                )))
                .await;
            return Ok(());
        }
    };

    // Verify session JWT token with deployment's public key using ES256 (same as frontend API)
    let claims = match verify_token::<SessionClaims>(&token, "ES256", &public_key) {
        Ok(token_data) => token_data.claims,
        Err(e) => {
            error!("Invalid session token for notification stream: {}", e);
            let error_msg = json!({
                "error": "Unauthorized - invalid session"
            });
            let _ = ws
                .write_frame(Frame::text(fastwebsockets::Payload::Owned(
                    serde_json::to_vec(&error_msg).unwrap(),
                )))
                .await;
            return Ok(());
        }
    };

    // Parse session_id from claims
    let session_id = match claims.sess.parse::<i64>() {
        Ok(id) => id,
        Err(e) => {
            error!("Invalid session_id in token: {}", e);
            let error_msg = json!({
                "error": "Invalid session ID"
            });
            let _ = ws
                .write_frame(Frame::text(fastwebsockets::Payload::Owned(
                    serde_json::to_vec(&error_msg).unwrap(),
                )))
                .await;
            return Ok(());
        }
    };

    // Query database to get user_id and active organization/workspace from session
    let session_context = match GetSessionWithActiveContextQuery::new(session_id)
        .execute(&app_state)
        .await
    {
        Ok(context) => context,
        Err(e) => {
            let error_msg = json!({
                "error": "Session not found or invalid"
            });
            let _ = ws
                .write_frame(Frame::text(fastwebsockets::Payload::Owned(
                    serde_json::to_vec(&error_msg).unwrap(),
                )))
                .await;
            return Ok(());
        }
    };

    let user_id = session_context.user_id;
    let active_organization_id = session_context.active_organization_id;
    let active_workspace_id = session_context.active_workspace_id;

    info!(
        "Notification WebSocket connection established for user: {}, deployment: {}",
        user_id, deployment_id
    );

    // Create NATS subject for this user
    let subject = format!("notifications.{}.{}", deployment_id, user_id);

    // Subscribe to NATS
    let mut subscriber = match app_state.nats_client.subscribe(subject.clone()).await {
        Ok(sub) => sub,
        Err(e) => {
            error!("Failed to subscribe to NATS subject {}: {}", subject, e);
            let error_msg = json!({
                "error": "Failed to subscribe to notifications"
            });
            let _ = ws
                .write_frame(Frame::text(fastwebsockets::Payload::Owned(
                    serde_json::to_vec(&error_msg).unwrap(),
                )))
                .await;
            return Ok(());
        }
    };

    info!(
        "User {} subscribed to notification subject: {}",
        user_id, subject
    );

    // Send initial connection success message
    let connected_msg = json!({
        "type": "connected",
        "message": "Notification stream connected"
    });
    ws.write_frame(Frame::text(fastwebsockets::Payload::Owned(
        serde_json::to_vec(&connected_msg).unwrap(),
    )))
    .await?;

    // Main loop: Listen to NATS and forward to WebSocket
    loop {
        tokio::select! {
            // Handle incoming NATS messages
            Some(msg) = subscriber.next() => {
                match serde_json::from_slice::<NotificationMessage>(&msg.payload) {
                    Ok(notification) => {
                        // Apply channel filtering
                        if should_send_notification(&notification, &params, active_organization_id, active_workspace_id) {
                            let ws_message = json!({
                                "type": "notification",
                                "data": notification
                            });

                            if let Err(e) = ws.write_frame(Frame::text(fastwebsockets::Payload::Owned(
                                serde_json::to_vec(&ws_message).unwrap()
                            ))).await {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                    }
                }
            }

            // Handle incoming WebSocket messages (e.g., ping)
            frame = ws.read_frame() => {
                match frame {
                    Ok(Frame { opcode, payload, .. }) => {
                        match opcode {
                            OpCode::Text => {
                                if let Ok(text) = std::str::from_utf8(&payload) {
                                    if text == "ping" {
                                        ws.write_frame(Frame::text(fastwebsockets::Payload::Borrowed(b"pong"))).await?;
                                    }
                                }
                            }
                            OpCode::Close => {
                                break;
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

fn should_send_notification(
    notification: &NotificationMessage,
    params: &NotificationParams,
    active_organization_id: Option<i64>,
    active_workspace_id: Option<i64>,
) -> bool {
    // If no channels specified, default to "user" notifications only
    let channels = match &params.channels {
        Some(channels) if !channels.is_empty() => channels,
        _ => return true, // Default behavior: send user notifications
    };

    // Check each channel type
    for channel in channels {
        match channel.as_str() {
            "user" => {
                // User notifications are always sent (they're targeted to this specific user)
                return true;
            }
            "organization" => {
                // Organization notifications: check if notification has organization_id
                if notification.organization_id.is_some() && notification.workspace_id.is_none() {
                    // Organization-level notification (has org_id but no workspace_id)
                    if let Some(org_ids) = &params.organization_ids {
                        if let Some(notif_org_id) = notification.organization_id {
                            if org_ids.contains(&notif_org_id) {
                                return true;
                            }
                        }
                    } else {
                        // No specific org IDs filter, include all org notifications
                        return true;
                    }
                }
            }
            "workspace" => {
                // Workspace notifications: check if notification has workspace_id
                if notification.workspace_id.is_some() {
                    if let Some(ws_ids) = &params.workspace_ids {
                        if let Some(notif_ws_id) = notification.workspace_id {
                            if ws_ids.contains(&notif_ws_id) {
                                return true;
                            }
                        }
                    } else {
                        // No specific workspace IDs filter, include all workspace notifications
                        return true;
                    }
                }
            }
            "current" => {
                // Current channel: notifications for the user's active context
                if let Some(active_org_id) = active_organization_id {
                    // Check if notification is for the active organization
                    if notification.organization_id == Some(active_org_id) {
                        if let Some(active_ws_id) = active_workspace_id {
                            // If there's an active workspace, show notifications for that workspace or org-level ones
                            if notification.workspace_id == Some(active_ws_id)
                                || notification.workspace_id.is_none()
                            {
                                return true;
                            }
                        } else {
                            // No active workspace, show org-level notifications
                            if notification.workspace_id.is_none() {
                                return true;
                            }
                        }
                    }
                }
                // Always include user-level notifications in current context
                if notification.organization_id.is_none() && notification.workspace_id.is_none() {
                    return true;
                }
            }
            _ => {
                // Unknown channel type, ignore
                continue;
            }
        }
    }

    false
}
