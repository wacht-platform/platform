use async_nats::jetstream;
use async_nats::jetstream::stream;
use axum::extract::{Query as QueryParams, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use common::state::AppState;
use common::utils::jwt::verify_agent_context_token;
use fastwebsockets::FragmentCollector;
use fastwebsockets::Frame;
use fastwebsockets::OpCode;
use fastwebsockets::WebSocketError;
use fastwebsockets::upgrade;
use futures::StreamExt;
use models::{ConversationContent, ConversationRecord, ExecutionContextStatus};
use queries::GetRecentConversationsQuery;
use queries::{GetAiAgentByNameWithFeatures, GetDeploymentWithKeyPairQuery};
use queries::{GetExecutionContextQuery, Query};
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::sync::{Mutex, mpsc};
use tracing::{error, warn};

use super::models::{WebsocketMessage, WebsocketMessageType};
use super::session::SessionState;
use crate::api::agent::{AgentHandler, ExecutionRequest};
use std::collections::HashMap;

pub async fn realtime_agent_handler(
    headers: HeaderMap,
    QueryParams(params): QueryParams<HashMap<String, String>>,
    ws: upgrade::IncomingUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    if host.is_none() {
        warn!("WebSocket connection attempted without Host header");
        return axum::response::Response::builder()
            .status(400)
            .body(axum::body::Body::from("Missing Host header"))
            .unwrap()
            .into_response();
    }

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
        if let Err(e) = handle_client(fut, state, host.unwrap(), token).await {
            eprintln!("Error in websocket connection: {e}");
        }
    });

    response.into_response()
}

async fn handle_client(
    fut: upgrade::UpgradeFut,
    app_state: AppState,
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
            // Send error and close connection
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

    // Verify JWT token with agent_context scope
    // Using ES256 which is the default algorithm for deployment tokens
    let claims = match verify_agent_context_token(&token, "ES256", &private_key, None) {
        Ok(claims) => claims,
        Err(e) => {
            error!("Failed to verify token for host {}: {}", host, e);
            let error_msg = json!({
                "error": "Invalid authentication token"
            });
            let _ = ws
                .write_frame(Frame::text(fastwebsockets::Payload::Owned(
                    serde_json::to_vec(&error_msg).unwrap(),
                )))
                .await;
            return Ok(());
        }
    };

    let user_id = claims.sub.clone();

    tracing::info!(
        "WebSocket connection for deployment {} (host: {}, user: {:?})",
        deployment_id,
        host,
        user_id
    );

    let (sender, mut receiver) = mpsc::unbounded_channel::<WebsocketMessage<Value>>();
    let session = Arc::new(Mutex::new(
        SessionState::new(sender.clone(), app_state.clone(), deployment_id)
            .with_user(user_id.clone()),
    ));
    let channel_ready = Arc::new(Notify::new());

    tokio::spawn({
        let session = session.clone();
        let channel_ready = channel_ready.clone();

        async move {
            let session_ready = {
                let session = session.lock().await;
                Arc::clone(&session.ready)
            };

            let close = {
                let session = session.lock().await;
                Arc::clone(&session.close)
            };

            channel_ready.notify_waiters();
            session_ready.notified().await;

            let context_id = {
                let session = session.lock().await;
                session.context_id.unwrap()
            };

            let consumer_stream = app_state
                .nats_jetstream
                .get_or_create_stream(jetstream::stream::Config {
                    name: "agent_execution_stream".to_string(),
                    subjects: vec!["agent_execution_stream.>".to_string()],
                    retention: stream::RetentionPolicy::WorkQueue,
                    ..Default::default()
                })
                .await
                .unwrap();

            let sid = app_state.sf.next_id().unwrap();

            let msg_consumer = consumer_stream
                .get_or_create_consumer(
                    &format!("receiver-{sid}"),
                    jetstream::consumer::pull::Config {
                        name: Some(format!("receiver-{sid}")),
                        filter_subject: format!("agent_execution_stream.context:{context_id}"),
                        inactive_threshold: Duration::from_secs(20),
                        ack_wait: Duration::from_secs(5), // Faster acknowledgment timeout
                        deliver_policy: jetstream::consumer::DeliverPolicy::New, // Only new messages from now
                        ..Default::default()
                    },
                )
                .await
                .unwrap();

            let mut msg_stream = msg_consumer.messages().await.unwrap();

            loop {
                tokio::select! {
                    msg = msg_stream.next() => {
                        match msg {
                            Some(Ok(message)) => {
                                // Get message type from headers
                                let message_type_header = message.headers
                                    .as_ref()
                                    .and_then(|h| h.get("message_type"))
                                    .map(|v| v.as_str());

                                match message_type_header {
                                    Some("conversation_message") => {
                                        match serde_json::from_slice::<Value>(&message.payload) {
                                            Ok(chunk) => {
                                                let _ = sender.send(WebsocketMessage {
                                                    message_id: None,
                                                    message_type: WebsocketMessageType::ConversationMessage,
                                                    data: chunk,
                                                });
                                            }
                                            Err(e) => {
                                                error!("Failed to deserialize conversation message: {}", e);
                                            }
                                        }
                                    }
                                    Some("platform_event") => {
                                        match serde_json::from_slice::<Value>(&message.payload) {
                                            Ok(event_data) => {
                                                let _ = sender.send(WebsocketMessage {
                                                    message_id: None,
                                                    message_type: WebsocketMessageType::PlatformEvent,
                                                    data: event_data,
                                                });
                                            }
                                            Err(e) => {
                                                error!("Failed to deserialize platform event: {}", e);
                                            }
                                        }
                                    }
                                    Some("platform_function") => {
                                        match serde_json::from_slice::<Value>(&message.payload) {
                                            Ok(function_data) => {
                                                let _ = sender.send(WebsocketMessage {
                                                    message_id: None,
                                                    message_type: WebsocketMessageType::PlatformFunction,
                                                    data: function_data,
                                                });
                                            }
                                            Err(e) => {
                                                error!("Failed to deserialize platform function: {}", e);
                                            }
                                        }
                                    }
                                    Some("user_input_request") => {
                                        match serde_json::from_slice::<ConversationContent>(&message.payload) {
                                            Ok(user_input_content) => {
                                                let _ = sender.send(WebsocketMessage {
                                                    message_id: None,
                                                    message_type: WebsocketMessageType::UserInputRequest,
                                                    data: serde_json::to_value(user_input_content).unwrap_or(serde_json::Value::Null),
                                                });
                                            }
                                            Err(e) => {
                                                error!("Failed to deserialize user input request: {}", e);
                                            }
                                        }
                                    }
                                    _ => {
                                        error!("Unknown message type in headers: {:?}", message_type_header);
                                    }
                                }
                                let _ = message.ack().await;
                            }
                            Some(Err(e)) => {
                                error!("Error receiving message: {}", e);
                            }
                            None => {
                                break;
                            }
                        }
                    }
                    _ = close.notified() => {
                        break;
                    }
                }
            }
        }
    });

    channel_ready.notified().await;

    loop {
        tokio::select! {
            Ok(frame) = ws.read_frame() => {
                let close = handler_websocket_message(frame,  session.clone());
                if close {
                    let session = session.lock().await;
                    session.close.notify_waiters();
                    break;
                }
            },
            Some(message) = receiver.recv() => {
                let payload = serde_json::to_vec(&message).unwrap();
                if let Err(e) = ws.write_frame(Frame::text(fastwebsockets::Payload::Owned(payload))).await {
                    eprintln!("Error writing frame: {e}");
                    break;
                }
                if message.message_type == WebsocketMessageType::CloseConnection {
                    let session = session.lock().await;
                    session.close.notify_waiters();
                    break;
                }
            }
        }
    }

    Ok(())
}

fn handler_websocket_message(frame: Frame, session_state: Arc<Mutex<SessionState>>) -> bool {
    match frame.opcode {
        OpCode::Close => true,
        OpCode::Text => {
            match serde_json::from_slice::<WebsocketMessage<Value>>(&frame.payload) {
                Ok(message) => {
                    tokio::spawn(handle_execution_message(message, session_state));
                }
                Err(e) => {
                    eprintln!("Error parsing message: {e}");
                }
            };

            false
        }
        _ => false,
    }
}

async fn handle_execution_message(
    message: WebsocketMessage<Value>,
    session_state: Arc<Mutex<SessionState>>,
) {
    let (deployment_id, sender, app_state) = {
        let state = session_state.lock().await;
        (
            state.deployment_id,
            state.sender.clone(),
            state.app_state.clone(),
        )
    };

    if let WebsocketMessageType::SessionConnect(context_id, agent_name) = message.message_type {
        let message =
            match GetExecutionContextQuery::new(context_id.parse().unwrap(), deployment_id)
                .execute(&app_state)
                .await
            {
                Ok(context) => {
                    // Map ExecutionContextStatus to frontend ExecutionStatus
                    let execution_status = match context.status {
                        ExecutionContextStatus::Idle => "Idle",
                        ExecutionContextStatus::Running => "Running",
                        ExecutionContextStatus::WaitingForInput => "WaitingForInput",
                        ExecutionContextStatus::Interrupted => "Failed",
                        ExecutionContextStatus::Completed => "Completed",
                        ExecutionContextStatus::Failed => "Failed",
                    };

                    WebsocketMessage {
                        message_id: message.message_id,
                        message_type: WebsocketMessageType::SessionConnected,
                        data: json!({
                            "context": context,
                            "execution_status": execution_status,
                        }),
                    }
                }
                Err(e) => WebsocketMessage {
                    message_id: message.message_id,
                    message_type: WebsocketMessageType::CloseConnection,
                    data: json!({
                        "error": format!("Failed to retrieve execution contexts: {}", e)
                    }),
                },
            };

        if message.message_type == WebsocketMessageType::CloseConnection {
            let _ = sender.send(message);
            return;
        }

        match GetAiAgentByNameWithFeatures::new(deployment_id, agent_name)
            .execute(&app_state)
            .await
        {
            Ok(agent) => {
                let mut session = session_state.lock().await;
                session.agent = Some(agent.clone());
                session.context_id = Some(context_id.parse().unwrap());

                // Notify the consumer to start BEFORE sending the connected message
                // This ensures the consumer is ready to receive messages
                session.ready.notify_waiters();

                // Small delay to ensure consumer is fully initialized
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

                let _ = sender.send(message);
            }
            Err(e) => {
                let message = WebsocketMessage {
                    message_id: message.message_id,
                    message_type: WebsocketMessageType::CloseConnection,
                    data: json!({
                        "error": format!("Failed to retrieve agent: {}", e)
                    }),
                };
                let _ = sender.send(message);
            }
        };

        return;
    }

    let (context_id, agent) = {
        let state = session_state.lock().await;

        if state.context_id.is_none() || state.agent.is_none() {
            let message = WebsocketMessage {
                message_id: message.message_id,
                message_type: WebsocketMessageType::CloseConnection,
                data: json!({
                    "error": "Context or agent not found"
                }),
            };
            let _ = sender.send(message);
            return;
        }

        (state.context_id.unwrap(), state.agent.clone().unwrap())
    };

    match message.message_type {
        WebsocketMessageType::FetchContextMessages => {
            let message = match GetRecentConversationsQuery::new(context_id, 100)
                .execute(&app_state)
                .await
            {
                Ok(messages) => WebsocketMessage {
                    message_id: message.message_id,
                    message_type: WebsocketMessageType::FetchContextMessages,
                    data: json!(messages),
                },
                Err(_) => WebsocketMessage {
                    message_id: message.message_id,
                    message_type: WebsocketMessageType::FetchContextMessages,
                    data: json!(Vec::<ConversationRecord>::new()),
                },
            };

            let _ = sender.send(message);
        }
        WebsocketMessageType::MessageInput(user_message) => {
            // Send starting status
            let status_message = WebsocketMessage {
                message_id: None,
                message_type: WebsocketMessageType::ExecutionStatusUpdate,
                data: json!({
                    "status": "Starting"
                }),
            };
            let _ = sender.send(status_message);

            let execution_request = ExecutionRequest {
                agent,
                user_message: Some(user_message),
                context_id,
                platform_function_result: None,
            };

            // Execute agent and update status based on result
            match AgentHandler::new(app_state)
                .execute_agent_streaming(execution_request)
                .await
            {
                Ok(_) => {
                    // Send idle status on completion
                    let status_message = WebsocketMessage {
                        message_id: None,
                        message_type: WebsocketMessageType::ExecutionStatusUpdate,
                        data: json!({
                            "status": "Idle"
                        }),
                    };
                    let _ = sender.send(status_message);
                }
                Err(_) => {
                    // Send failed status on error
                    let status_message = WebsocketMessage {
                        message_id: None,
                        message_type: WebsocketMessageType::ExecutionStatusUpdate,
                        data: json!({
                            "status": "Failed"
                        }),
                    };
                    let _ = sender.send(status_message);
                }
            }
        }
        WebsocketMessageType::PlatformFunctionResult(execution_id, result) => {
            tracing::info!(
                "Received platform function result for execution_id: {}, result: {:?}",
                execution_id,
                result
            );

            if let Ok(context) = GetExecutionContextQuery::new(context_id, deployment_id)
                .execute(&app_state)
                .await
            {
                let status_str = match context.status {
                    ExecutionContextStatus::Idle => "idle",
                    ExecutionContextStatus::Running => "running",
                    ExecutionContextStatus::WaitingForInput => "waiting_for_input",
                    ExecutionContextStatus::Interrupted => "interrupted",
                    ExecutionContextStatus::Completed => "completed",
                    ExecutionContextStatus::Failed => "failed",
                };
                tracing::info!(
                    "Current context status: {}, has execution_state: {}",
                    status_str,
                    context.execution_state.is_some()
                );

                // If the context was in WaitingForInput state, resume execution
                if matches!(context.status, ExecutionContextStatus::WaitingForInput) {
                    if context.execution_state.is_some() {
                        tracing::info!("Context was waiting for input, resuming agent execution");

                        // Get the agent from session state
                        let session = session_state.lock().await;
                        if let Some(agent) = session.agent.clone() {
                            drop(session); // Release the lock before calling execute_agent_streaming

                            // Create resume request with platform function result
                            let resume_request = ExecutionRequest {
                                agent,
                                user_message: None, // No user message for platform function resume
                                context_id,
                                platform_function_result: Some((
                                    execution_id.clone(),
                                    result.clone(),
                                )),
                            };

                            // Execute agent directly (we're already in a spawned task)
                            let result = AgentHandler::new(app_state.clone())
                                .execute_agent_streaming(resume_request)
                                .await;
                            tracing::info!("Agent resume completed: {:?}", result.is_ok());
                        } else {
                            tracing::error!("No agent found in session state");
                        }
                    } else {
                        tracing::warn!("No execution state found in context");
                    }
                }
            } else {
                tracing::error!(
                    "Failed to get execution context for context_id: {}, deployment_id: {}",
                    context_id,
                    deployment_id
                );
            }
        }
        WebsocketMessageType::UserInputResponse(input) => {
            tracing::info!("Received user input response: {}", input);

            // Resume execution with user input
            let execution_request = ExecutionRequest {
                agent,
                user_message: Some(input), // Will be handled as user input in agent handler
                context_id,
                platform_function_result: None,
            };

            match AgentHandler::new(app_state)
                .execute_agent_streaming(execution_request)
                .await
            {
                Ok(_) => {
                    tracing::info!("Successfully resumed execution with user input");
                }
                Err(e) => {
                    tracing::error!("Failed to resume with user input: {}", e);
                    let status_message = WebsocketMessage {
                        message_id: None,
                        message_type: WebsocketMessageType::ExecutionStatusUpdate,
                        data: json!({
                            "status": "Failed"
                        }),
                    };
                    let _ = sender.send(status_message);
                }
            }
        }
        WebsocketMessageType::CancelExecution => {
            // Update context status to cancelled
            use commands::{Command, UpdateExecutionContextQuery};

            let _ = UpdateExecutionContextQuery::new(context_id, deployment_id)
                .with_status(ExecutionContextStatus::Failed)
                .execute(&app_state)
                .await;

            // Send cancellation confirmation
            let message = WebsocketMessage {
                message_id: message.message_id,
                message_type: WebsocketMessageType::ExecutionCancelled,
                data: json!({}),
            };

            let _ = sender.send(message);
        }
        _ => {}
    };
}
