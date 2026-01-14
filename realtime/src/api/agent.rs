use super::models::{WebsocketMessage, WebsocketMessageType};
use super::session::SessionState;
use crate::middleware::host_extractor::ExtractedHost;
use async_nats::jetstream;
use async_nats::jetstream::stream;
use axum::Extension;
use axum::extract::{Query as QueryParams, State};
use axum::response::IntoResponse;
use commands::agent_execution::{PublishAgentExecutionCommand, UploadImagesToS3Command};
use common::state::AppState;
use common::utils::jwt::verify_agent_context_token;
use dto::json::{ExecutionStatusUpdate, SessionConnectedMessage, WebSocketError};
use fastwebsockets::FragmentCollector;
use fastwebsockets::Frame;
use fastwebsockets::OpCode;
use fastwebsockets::WebSocketError as FastWebSocketError;
use fastwebsockets::upgrade;
use futures::StreamExt;
use models::ExecutionContextStatus;
use models::{ConversationContent, ConversationRecord};
use queries::GetRecentConversationsQuery;
use queries::{GetAiAgentByNameWithFeatures, GetDeploymentWithKeyPairQuery};
use queries::{GetExecutionContextQuery, Query};
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::sync::{Mutex, mpsc};
use tracing::{error, warn};

pub async fn agent_stream_handler(
    Extension(ExtractedHost(host)): Extension<ExtractedHost>,
    QueryParams(params): QueryParams<HashMap<String, String>>,
    ws: upgrade::IncomingUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let session_id = params.get("session_id").cloned();
    if session_id.is_none() {
        warn!("WebSocket connection attempted without session_id");
        return axum::response::Response::builder()
            .status(401)
            .body(axum::body::Body::from("session_id required"))
            .unwrap()
            .into_response();
    }

    let session_id = session_id.unwrap();

    let (response, fut) = ws.upgrade().unwrap();

    tokio::task::spawn(
        async move { if let Err(_e) = handle_client(fut, state, host, session_id).await {} },
    );

    response.into_response()
}

async fn handle_client(
    fut: upgrade::UpgradeFut,
    app_state: AppState,
    host: String,
    session_id_str: String,
) -> Result<(), FastWebSocketError> {
    let mut ws = FragmentCollector::new(fut.await?);

    let (deployment_id, public_key) = match GetDeploymentWithKeyPairQuery::new(host.clone())
        .execute(&app_state)
        .await
    {
        Ok(result) => result,
        Err(e) => {
            error!("Failed to get deployment for host {}: {}", host, e);
            let error_msg = WebSocketError {
                error: "Invalid deployment".to_string(),
            };
            let _ = ws
                .write_frame(Frame::text(fastwebsockets::Payload::Owned(
                    serde_json::to_vec(&error_msg).unwrap(),
                )))
                .await;
            return Ok(());
        }
    };

    let session_id: i64 = match session_id_str.parse() {
        Ok(id) => id,
        Err(_) => {
            warn!("Invalid session_id format: {}", session_id_str);
            let error_msg = WebSocketError {
                error: "Invalid session ID".to_string(),
            };
            let _ = ws
                .write_frame(Frame::text(fastwebsockets::Payload::Owned(
                    serde_json::to_vec(&error_msg).unwrap(),
                )))
                .await;
            return Ok(());
        }
    };

    let agent_session = match queries::GetAgentSessionQuery::new(session_id, deployment_id as i64)
        .execute(&app_state)
        .await
    {
        Ok(Some(session)) => session,
        Ok(None) => {
            warn!("No active agent session for session_id {} on deployment {}", session_id, deployment_id);
            let error_msg = WebSocketError {
                error: "No active agent session. Please exchange your ticket first.".to_string(),
            };
            let _ = ws
                .write_frame(Frame::text(fastwebsockets::Payload::Owned(
                    serde_json::to_vec(&error_msg).unwrap(),
                )))
                .await;
            return Ok(());
        }
        Err(e) => {
            error!("Failed to query agent session: {}", e);
            let error_msg = WebSocketError {
                error: "Failed to verify agent session".to_string(),
            };
            let _ = ws
                .write_frame(Frame::text(fastwebsockets::Payload::Owned(
                    serde_json::to_vec(&error_msg).unwrap(),
                )))
                .await;
            return Ok(());
        }
    };

    if !agent_session.is_active() {
        warn!("Agent session {} is expired or deleted", agent_session.id);
        let error_msg = WebSocketError {
            error: "Agent session expired".to_string(),
        };
        let _ = ws
            .write_frame(Frame::text(fastwebsockets::Payload::Owned(
                serde_json::to_vec(&error_msg).unwrap(),
            )))
            .await;
        return Ok(());
    }

    let user_id = session_id.to_string();
    let context_group = agent_session.context_group.clone();
    
    let audience = if !agent_session.agent_ids.is_empty() {
        Some(agent_session.agent_ids[0].to_string())
    } else {
        None
    };

    tracing::info!(
        "WebSocket connection for deployment {} (host: {}, session: {}, context: {})",
        deployment_id,
        host,
        session_id,
        context_group
    );

    let (sender, mut receiver) = mpsc::unbounded_channel::<WebsocketMessage<Value>>();
    let session = Arc::new(Mutex::new(
        SessionState::new(sender.clone(), app_state.clone(), deployment_id)
            .with_user(Some(user_id.clone()))
            .with_audience(audience),
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
                        ack_wait: Duration::from_secs(5),
                        deliver_policy: jetstream::consumer::DeliverPolicy::New,
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
                                let error_str = e.to_string();
                                if error_str.contains("heartbeat") || error_str.contains("responders") {
                                    warn!("NATS stream connection lost ({}), cleaning up consumer task", error_str);
                                    break;
                                }
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
                if let Err(_) = ws.write_frame(Frame::text(fastwebsockets::Payload::Owned(payload))).await {
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
                Err(_e) => {}
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
        match GetExecutionContextQuery::new(context_id.parse().unwrap(), deployment_id)
            .execute(&app_state)
            .await
        {
            Ok(context) => {
                if let Some(token_audience) = &session_state.lock().await.audience {
                    if let Some(ref context_group) = context.context_group {
                        if token_audience != context_group {
                            let error = WebSocketError {
                                error: format!(
                                    "Access denied: token audience '{}' does not match execution context group '{}'",
                                    token_audience, context_group
                                ),
                            };
                            let error_message = WebsocketMessage {
                                message_id: message.message_id,
                                message_type: WebsocketMessageType::CloseConnection,
                                data: serde_json::to_value(&error).unwrap(),
                            };
                            let _ = sender.send(error_message);
                            return;
                        }
                    } else {
                        let error_message = WebsocketMessage {
                            message_id: message.message_id,
                            message_type: WebsocketMessageType::CloseConnection,
                            data: json!({
                                "error": format!("Access denied: token requires audience '{}' but execution context has no group", token_audience)
                            }),
                        };
                        let _ = sender.send(error_message);
                        return;
                    }
                }

                let execution_status = match context.status {
                    ExecutionContextStatus::Idle => "Idle",
                    ExecutionContextStatus::Running => "Running",
                    ExecutionContextStatus::WaitingForInput => "WaitingForInput",
                    ExecutionContextStatus::Interrupted => "Failed",
                    ExecutionContextStatus::Completed => "Completed",
                    ExecutionContextStatus::Failed => "Failed",
                };

                match GetAiAgentByNameWithFeatures::new(deployment_id, agent_name)
                    .execute(&app_state)
                    .await
                {
                    Ok(agent) => {
                        let quick_questions: Option<Vec<String>> = agent
                            .configuration
                            .get("quick_questions")
                            .and_then(|v| serde_json::from_value(v.clone()).ok());

                        let session_data = SessionConnectedMessage {
                            context: serde_json::to_value(&context).unwrap(),
                            execution_status: execution_status.to_string(),
                            quick_questions,
                        };
                        let msg = WebsocketMessage {
                            message_id: message.message_id,
                            message_type: WebsocketMessageType::SessionConnected,
                            data: serde_json::to_value(&session_data).unwrap(),
                        };

                        {
                            let mut session = session_state.lock().await;
                            session.agent = Some(agent.clone());
                            session.context_id = Some(context_id.parse().unwrap());
                            session.ready.notify_waiters();
                        }

                        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

                        let _ = sender.send(msg);
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
            }
            Err(e) => {
                let message = WebsocketMessage {
                    message_id: message.message_id,
                    message_type: WebsocketMessageType::CloseConnection,
                    data: json!({
                        "error": format!("Failed to retrieve execution contexts: {}", e)
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
            let user_images = message.data.get("images").and_then(|v| {
                serde_json::from_value::<Vec<dto::json::agent_executor::ImageData>>(v.clone()).ok()
            });

            let status_update = ExecutionStatusUpdate {
                status: "Starting".to_string(),
            };
            let status_message = WebsocketMessage {
                message_id: None,
                message_type: WebsocketMessageType::ExecutionStatusUpdate,
                data: serde_json::to_value(&status_update).unwrap(),
            };
            let _ = sender.send(status_message);

            use commands::{Command, CreateConversationCommand};
            use models::{ConversationContent, ConversationMessageType};

            let model_images =
                match UploadImagesToS3Command::new(deployment_id, context_id, user_images)
                    .execute(&app_state)
                    .await
                {
                    Ok(images) => images,
                    Err(e) => {
                        tracing::error!("Failed to upload images: {}", e);
                        None
                    }
                };

            let conversation_id = app_state.sf.next_id().unwrap_or(0) as i64;
            let create_result = CreateConversationCommand::new(
                conversation_id,
                context_id,
                ConversationContent::UserMessage {
                    message: user_message,
                    sender_name: None,
                    images: model_images,
                },
                ConversationMessageType::UserMessage,
            )
            .execute(&app_state)
            .await;

            if let Err(e) = create_result {
                tracing::error!("Failed to create conversation: {}", e);
                let status_update = ExecutionStatusUpdate {
                    status: "Failed".to_string(),
                };
                let status_message = WebsocketMessage {
                    message_id: None,
                    message_type: WebsocketMessageType::ExecutionStatusUpdate,
                    data: serde_json::to_value(&status_update).unwrap(),
                };
                let _ = sender.send(status_message);
                return;
            }

            if let Err(e) = PublishAgentExecutionCommand::new_message(
                deployment_id,
                context_id,
                Some(agent.id),
                None,
                conversation_id,
            )
            .execute(&app_state)
            .await
            {
                tracing::error!("Failed to publish execution request: {}", e);
                let status_update = ExecutionStatusUpdate {
                    status: "Failed".to_string(),
                };
                let status_message = WebsocketMessage {
                    message_id: None,
                    message_type: WebsocketMessageType::ExecutionStatusUpdate,
                    data: serde_json::to_value(&status_update).unwrap(),
                };
                let _ = sender.send(status_message);
                return;
            }

            tracing::info!(
                "Published agent execution request for context {}",
                context_id
            );
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

                // If the context was in WaitingForInput state, publish resume request to NATS
                if matches!(context.status, ExecutionContextStatus::WaitingForInput) {
                    if context.execution_state.is_some() {
                        tracing::info!(
                            "Context was waiting for input, publishing resume request to NATS"
                        );

                        // Publish platform function result to NATS (worker will handle resume)
                        use commands::Command;

                        if let Err(e) = PublishAgentExecutionCommand::platform_function_result(
                            deployment_id,
                            context_id,
                            Some(agent.id),
                            None,
                            execution_id.clone(),
                            result.clone(),
                        )
                        .execute(&app_state)
                        .await
                        {
                            tracing::error!("Failed to publish platform function result: {}", e);
                        } else {
                            tracing::info!(
                                "Published platform function result for execution_id: {}",
                                execution_id
                            );
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

            use commands::{Command, CreateConversationCommand};
            use models::{ConversationContent, ConversationMessageType};

            let conversation_id = app_state.sf.next_id().unwrap_or(0) as i64;
            let create_result = CreateConversationCommand::new(
                conversation_id,
                context_id,
                ConversationContent::UserMessage {
                    message: input,
                    sender_name: None,
                    images: None,
                },
                ConversationMessageType::UserMessage,
            )
            .execute(&app_state)
            .await;

            if let Err(e) = create_result {
                tracing::error!("Failed to create user input conversation: {}", e);
                let status_update = ExecutionStatusUpdate {
                    status: "Failed".to_string(),
                };
                let status_message = WebsocketMessage {
                    message_id: None,
                    message_type: WebsocketMessageType::ExecutionStatusUpdate,
                    data: serde_json::to_value(&status_update).unwrap(),
                };
                let _ = sender.send(status_message);
                return;
            }

            if let Err(e) = PublishAgentExecutionCommand::user_input_response(
                deployment_id,
                context_id,
                Some(agent.id),
                None,
                conversation_id,
            )
            .execute(&app_state)
            .await
            {
                tracing::error!("Failed to publish user input response: {}", e);
                let status_update = ExecutionStatusUpdate {
                    status: "Failed".to_string(),
                };
                let status_message = WebsocketMessage {
                    message_id: None,
                    message_type: WebsocketMessageType::ExecutionStatusUpdate,
                    data: serde_json::to_value(&status_update).unwrap(),
                };
                let _ = sender.send(status_message);
            } else {
                tracing::info!("Published user input response for context {}", context_id);
            }
        }
        WebsocketMessageType::CancelExecution => {
            use commands::{Command, UpdateExecutionContextQuery};

            let _ = UpdateExecutionContextQuery::new(context_id, deployment_id)
                .with_status(ExecutionContextStatus::Failed)
                .execute(&app_state)
                .await;

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
