use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::IntoResponse,
};
use common::utils::jwt::verify_token;
use fastwebsockets::{FragmentCollector, Frame, OpCode, WebSocketError, upgrade};
use futures::StreamExt;
use queries::{GetDeploymentWithKeyPairQuery, Query as QueryTrait};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use tracing::{error, info, warn};

use crate::application::HttpState;

#[derive(Debug, Deserialize)]
pub struct NotificationParams {
    pub token: String,
    pub host: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NotificationClaims {
    pub sub: String,
    pub exp: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationMessage {
    pub id: i64,
    pub user_id: i64,
    pub deployment_id: i64,
    pub title: String,
    pub body: String,
    pub severity: String,
    pub action_url: Option<String>,
    pub action_label: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn notification_stream_handler(
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    ws: upgrade::IncomingUpgrade,
    State(state): State<HttpState>,
) -> impl IntoResponse {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "localhost".to_string());

    // Extract token from query parameters
    let token = params.get("token").cloned();

    if token.is_none() {
        warn!("WebSocket connection attempted without authentication token");
        return axum::response::Response::builder()
            .status(401)
            .body(axum::body::Body::from("Authentication required"))
            .unwrap()
            .into_response();
    }

    let token = token.unwrap();

    let (response, fut) = ws.upgrade().unwrap();

    tokio::task::spawn(async move {
        if let Err(e) = handle_notification_client(fut, state, host, token).await {
            error!("Error in notification websocket connection: {e}");
        }
    });

    response.into_response()
}

async fn handle_notification_client(
    fut: upgrade::UpgradeFut,
    app_state: HttpState,
    host: String,
    token: String,
) -> Result<(), WebSocketError> {
    let mut ws = FragmentCollector::new(fut.await?);

    // Get deployment ID and private key from host
    let (deployment_id, private_key) = match GetDeploymentWithKeyPairQuery::new(host.clone())
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

    // Verify JWT token with deployment's private key using ES256 (same as frontend API)
    let claims = match verify_token::<NotificationClaims>(&token, "ES256", &private_key) {
        Ok(token_data) => token_data.claims,
        Err(e) => {
            error!("Invalid JWT token for notification stream: {}", e);
            let error_msg = json!({
                "error": "Unauthorized"
            });
            let _ = ws
                .write_frame(Frame::text(fastwebsockets::Payload::Owned(
                    serde_json::to_vec(&error_msg).unwrap(),
                )))
                .await;
            return Ok(());
        }
    };

    // Parse user_id from string to i64
    let user_id = match claims.sub.parse::<i64>() {
        Ok(id) => id,
        Err(e) => {
            error!("Invalid user_id in token: {}", e);
            let error_msg = json!({
                "error": "Invalid user ID"
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
                        let ws_message = json!({
                            "type": "notification",
                            "data": notification
                        });

                        if let Err(e) = ws.write_frame(Frame::text(fastwebsockets::Payload::Owned(
                            serde_json::to_vec(&ws_message).unwrap()
                        ))).await {
                            error!("Failed to send notification to WebSocket: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse notification message: {}", e);
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
                                info!("WebSocket closed by client for user {}", user_id);
                                break;
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        error!("WebSocket error for user {}: {}", user_id, e);
                        break;
                    }
                }
            }
        }
    }

    info!("Notification stream closed for user {}", user_id);
    Ok(())
}
