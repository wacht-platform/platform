use crate::realtime::middleware::host_extractor::ExtractedHost;
use async_nats::jetstream;
use async_nats::jetstream::consumer::PullConsumer;
use axum::Extension;
use axum::body::Body;
use axum::extract::{Query as QueryParams, State};
use axum::http::{HeaderMap, header, header::COOKIE};
use axum::response::{IntoResponse, Response};
use common::state::AppState;
use common::utils::jwt::verify_token;
use dto::json::{AgentStreamMessageType, StreamEvent};
use futures::StreamExt;
use models::ConversationContent;
use queries::{GetAgentSessionQuery, GetDeploymentWithKeyPairQuery, Query};
use serde::Deserialize;
use std::str::FromStr;
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{error, info, warn};

#[derive(Debug, Deserialize)]
pub struct SSEParams {
    pub context_id: Option<String>,
    #[serde(rename = "__dev_session__")]
    pub dev_session: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SessionClaims {
    pub sess: String,
}

fn extract_session_token(headers: &HeaderMap, params: &SSEParams) -> Option<String> {
    if let Some(cookie_header) = headers.get(COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
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
    params.dev_session.clone()
}

pub async fn agent_sse_stream_handler(
    Extension(ExtractedHost(host)): Extension<ExtractedHost>,
    headers: HeaderMap,
    QueryParams(params): QueryParams<SSEParams>,
    State(app_state): State<AppState>,
) -> impl IntoResponse {
    let context_id = match &params.context_id {
        Some(id) => id.clone(),
        None => {
            return Response::builder()
                .status(400)
                .body(Body::from("context_id required"))
                .unwrap();
        }
    };

    let session_token = match extract_session_token(&headers, &params) {
        Some(token) => token,
        None => {
            warn!("SSE connection attempted without session token");
            return Response::builder()
                .status(401)
                .body(Body::from("Session token required"))
                .unwrap();
        }
    };

    let (deployment_id, public_key) = match GetDeploymentWithKeyPairQuery::new(host.clone())
        .execute(&app_state)
        .await
    {
        Ok(result) => result,
        Err(e) => {
            error!("Failed to get deployment for host {}: {}", host, e);
            return Response::builder()
                .status(500)
                .body(Body::from("Failed to get deployment"))
                .unwrap();
        }
    };

    let claims = match verify_token::<SessionClaims>(&session_token, "ES256", &public_key) {
        Ok(token_data) => token_data.claims,
        Err(e) => {
            error!("Invalid session token for SSE stream: {}", e);
            return Response::builder()
                .status(401)
                .body(Body::from("Unauthorized - invalid session"))
                .unwrap();
        }
    };

    let session_id: i64 = match claims.sess.parse() {
        Ok(id) => id,
        Err(e) => {
            error!("Invalid session_id in token: {}", e);
            return Response::builder()
                .status(401)
                .body(Body::from("Invalid session ID"))
                .unwrap();
        }
    };

    let _agent_session = match GetAgentSessionQuery::new(session_id, deployment_id as i64)
        .execute(&app_state)
        .await
    {
        Ok(Some(session)) if session.is_active() => session,
        Ok(Some(_)) => {
            warn!("Agent session expired for session_id {}", session_id);
            return Response::builder()
                .status(401)
                .body(Body::from("Agent session expired"))
                .unwrap();
        }
        Ok(None) => {
            warn!("No agent session for session_id {}", session_id);
            return Response::builder()
                .status(401)
                .body(Body::from("No active agent session"))
                .unwrap();
        }
        Err(e) => {
            error!("Failed to query agent session: {}", e);
            return Response::builder()
                .status(500)
                .body(Body::from("Failed to verify session"))
                .unwrap();
        }
    };

    info!(
        "SSE stream for context {} (session: {}, deployment: {})",
        context_id, session_id, deployment_id
    );

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, std::io::Error>>(100);

    let nats_client = app_state.nats_jetstream.clone();
    let ctx_id = context_id.clone();
    let app_state_clone = app_state.clone();

    tokio::spawn(async move {
        if let Err(e) = subscribe_and_stream(nats_client, ctx_id, tx, app_state_clone).await {
            error!("SSE stream error: {}", e);
        }
    });

    let stream = ReceiverStream::new(rx);
    let body = Body::from_stream(stream);

    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .header("X-Accel-Buffering", "no")
        .body(body)
        .unwrap()
}

async fn subscribe_and_stream(
    js: jetstream::Context,
    context_id: String,
    tx: tokio::sync::mpsc::Sender<Result<String, std::io::Error>>,
    app_state: AppState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("SSE: Creating/getting stream for context {}", context_id);

    let stream = js
        .get_or_create_stream(jetstream::stream::Config {
            name: "agent_execution_stream".to_string(),
            subjects: vec!["agent_execution_stream.>".to_string()],
            ..Default::default()
        })
        .await?;

    let consumer_id = app_state.sf.next_id().unwrap_or(0);
    let consumer_name = format!("sse_consumer_{}", consumer_id);

    info!(
        "SSE: Creating consumer {} for context {}",
        consumer_name, context_id
    );

    let consumer: PullConsumer = stream
        .create_consumer(jetstream::consumer::pull::Config {
            name: Some(consumer_name.clone()),
            filter_subject: format!("agent_execution_stream.context:{}", context_id),
            inactive_threshold: Duration::from_secs(300), // Increased from 60 to 300 seconds
            ack_wait: Duration::from_secs(30),
            deliver_policy: jetstream::consumer::DeliverPolicy::New,
            ..Default::default()
        })
        .await?;

    info!(
        "SSE: Consumer {} created, sending connected event",
        consumer_name
    );

    let connected_event = format!(
        "event: connected\ndata: {}\n\n",
        serde_json::json!({"context_id": context_id})
    );
    if tx.send(Ok(connected_event)).await.is_err() {
        warn!("SSE: Failed to send connected event, client disconnected");
        return Ok(());
    }

    info!(
        "SSE: Starting message consumption for context {}",
        context_id
    );

    let mut messages = consumer.messages().await?;

    info!("SSE: Message stream established for context {}", context_id);

    while let Some(msg_result) = messages.next().await {
        match msg_result {
            Ok(message) => {
                let _ = message.ack().await;

                // Get message_type from headers
                let message_type = message
                    .headers
                    .as_ref()
                    .and_then(|h| h.get("message_type"))
                    .and_then(|v| AgentStreamMessageType::from_str(v.as_str()).ok());

                let Some(message_type) = message_type else {
                    warn!("Unknown message type in SSE stream");
                    continue;
                };

                let stream_event =
                    match dto::json::decode_stream_event(message_type, &message.payload) {
                        Ok(event) => event,
                        Err(e) => {
                            warn!("Failed to decode stream event: {}", e);
                            continue;
                        }
                    };

                if let StreamEvent::ConversationMessage(conv) = &stream_event {
                    // Filter conversation messages to only include displayable types
                    if !is_displayable_message_type(&conv.content) {
                        continue;
                    }
                }

                let event_type = stream_event.message_type().as_header_value();
                let payload = serde_json::to_string(&stream_event).unwrap_or_default();

                let sse_data = format!("event: {}\ndata: {}\n\n", event_type, payload);

                if tx.send(Ok(sse_data)).await.is_err() {
                    warn!("SSE: Client disconnected, closing stream");
                    break;
                }
            }
            Err(e) => {
                error!("NATS message error: {}", e);
                break;
            }
        }
    }

    info!("SSE: Message loop ended, stream closing");
    Ok(())
}

/// Check if a conversation content type should be sent to the frontend
/// Matches the allowed types in Go frontend API's GetContextMessages
fn is_displayable_message_type(content: &ConversationContent) -> bool {
    matches!(
        content,
        ConversationContent::UserMessage { .. }
            | ConversationContent::AgentResponse { .. }
            | ConversationContent::AssistantAcknowledgment { .. }
            | ConversationContent::SystemDecision { .. }
            | ConversationContent::UserInputRequest { .. }
    )
}
