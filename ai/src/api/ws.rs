use std::sync::Arc;

use axum::extract::State;
use axum::response::IntoResponse;
use fastwebsockets::FragmentCollector;
use fastwebsockets::Frame;
use fastwebsockets::OpCode;
use fastwebsockets::WebSocketError;
use fastwebsockets::upgrade;
use serde_json::json;
use shared::state::AppState;
use tokio::sync::{Mutex, mpsc};

use super::models::{ExecutionStatus, WebsocketMessage, WebsocketMessageType};
use super::session::SessionState;

pub async fn handler(
    ws: upgrade::IncomingUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let (response, fut) = ws.upgrade().unwrap();

    tokio::task::spawn(async move {
        if let Err(e) = handle_client(fut, state).await {
            eprintln!("Error in websocket connection: {}", e);
        }
    });

    response
}

async fn handle_client(
    fut: upgrade::UpgradeFut,
    app_state: AppState,
) -> Result<(), WebSocketError> {
    let mut ws = FragmentCollector::new(fut.await?);
    let (sender, mut receiver) = mpsc::unbounded_channel::<WebsocketMessage>();
    let session_state = Arc::new(Mutex::new(SessionState::new(sender, 0)));

    loop {
        tokio::select! {
            Ok(frame) = ws.read_frame() => {
                let close = handler_websocket_message(frame, app_state.clone(), session_state.clone());
                if close {
                    break;
                }
            },
            Some(message) = receiver.recv() => {
                let payload = serde_json::to_vec(&message).unwrap();
                if let Err(e) = ws.write_frame(Frame::binary(fastwebsockets::Payload::Owned(payload))).await {
                    eprintln!("Error writing frame: {}", e);
                    break;
                }
                if message.message_type == WebsocketMessageType::CloseConnection {
                    break;
                }
            }
        }
    }

    Ok(())
}

fn handler_websocket_message(
    frame: Frame,
    app_state: AppState,
    session_state: Arc<Mutex<SessionState>>,
) -> bool {
    match frame.opcode {
        OpCode::Close => true,
        OpCode::Binary => {
            let _ = match serde_json::from_slice::<WebsocketMessage>(&frame.payload) {
                Ok(message) => {
                    tokio::spawn(handle_execution_message(message, app_state, session_state));
                }
                Err(e) => {
                    eprintln!("Error parsing message: {}", e);
                }
            };

            false
        }
        _ => false,
    }
}

async fn handle_execution_message(
    message: WebsocketMessage,
    app_state: AppState,
    session_state: Arc<Mutex<SessionState>>,
) {
    use crate::api::agent::{AgentHandler, ExecutionRequest};
    use shared::queries::{GetExecutionContextsBySessionQuery, Query};

    match message.message_type {
        WebsocketMessageType::SessionReconnect(session_id) => {
            let mut state = session_state.lock().await;
            state.session_id = Some(session_id.clone());

            match GetExecutionContextsBySessionQuery::new(session_id.clone(), state.deployment_id)
                .execute(&app_state)
                .await
            {
                Ok(contexts) => {
                    let status_message = WebsocketMessage {
                        message_type: WebsocketMessageType::SessionStatus(session_id),
                        data: serde_json::to_vec(&json!({
                            "status": "reconnected",
                            "existing_contexts": contexts.len(),
                            "contexts": contexts
                        }))
                        .unwrap_or_default(),
                    };
                    let _ = state.sender.send(status_message);
                }
                Err(e) => {
                    eprintln!("Failed to load session contexts: {}", e);
                }
            }
        }
        WebsocketMessageType::RequestContext(agent_name) => {
            let (deployment_id, session_id) = {
                let state = session_state.lock().await;
                let session_id = state
                    .session_id
                    .clone()
                    .unwrap_or_else(|| format!("session_{}", chrono::Utc::now().timestamp()));
                (state.deployment_id, session_id)
            };

            let agent_handler = AgentHandler::new(app_state.clone());
            match agent_handler
                .get_or_create_context(&agent_name, deployment_id, &session_id)
                .await
            {
                Ok(context) => {
                    let response_message = WebsocketMessage {
                        message_type: WebsocketMessageType::RequestContextResponse(
                            context.id as u64,
                        ),
                        data: serde_json::to_vec(&json!({
                            "execution_context": context,
                            "agent_name": agent_name,
                            "session_id": session_id
                        }))
                        .unwrap_or_default(),
                    };
                    let state = session_state.lock().await;
                    let _ = state.sender.send(response_message);
                }
                Err(e) => {
                    eprintln!("Failed to get execution context: {}", e);
                }
            }
        }
        WebsocketMessageType::MessageInput(execution_id, user_input) => {
            let (deployment_id, _session_id, agent_name) = {
                let mut state = session_state.lock().await;

                let agent_name = if let Some(execution_info) =
                    state.active_executions.get(&execution_id)
                {
                    execution_info.agent_name.clone()
                } else {
                    match String::from_utf8(message.data.clone()) {
                        Ok(data) => {
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data) {
                                parsed
                                    .get("agent_name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("default_agent")
                                    .to_string()
                            } else {
                                "default_agent".to_string()
                            }
                        }
                        Err(_) => "default_agent".to_string(),
                    }
                };

                state.add_execution(execution_id, agent_name.clone());

                let session_id = state
                    .session_id
                    .clone()
                    .unwrap_or_else(|| format!("session_{}", execution_id));

                if state.session_id.is_none() {
                    state.session_id = Some(session_id.clone());
                }

                (state.deployment_id, session_id, agent_name)
            };

            // Send starting status
            {
                let state = session_state.lock().await;
                let starting_message = WebsocketMessage {
                    message_type: WebsocketMessageType::ExecutionUpdate(execution_id),
                    data: serde_json::to_vec(&json!({
                        "status": "starting",
                        "message": "Initializing agent execution..."
                    }))
                    .unwrap_or_default(),
                };
                let _ = state.sender.send(starting_message);
            }

            let agent_handler = AgentHandler::new(app_state.clone());
            let sender = {
                let state = session_state.lock().await;
                state.sender.clone()
            };

            {
                let mut state = session_state.lock().await;
                state.update_execution_status(execution_id, ExecutionStatus::Running);
            }

            let execution_request = ExecutionRequest {
                agent_name: agent_name.clone(),
                deployment_id,
                user_message: user_input.clone(),
                session_id: Some(format!("session_{}", execution_id)),
            };

            let execution_result = agent_handler
                .execute_agent_streaming(execution_request, move |chunk| {
                    let response_message = WebsocketMessage {
                        message_type: WebsocketMessageType::ExecutionUpdate(execution_id),
                        data: chunk.as_bytes().to_vec(),
                    };
                    let _ = sender.send(response_message);
                })
                .await;

            match execution_result {
                Ok(response) => {
                    let mut state = session_state.lock().await;
                    state.update_execution_status(execution_id, ExecutionStatus::Completed);

                    let completion_message = WebsocketMessage {
                        message_type: WebsocketMessageType::ExecutionComplete(execution_id),
                        data: serde_json::to_vec(&json!({
                            "status": "completed",
                            "message": "Agent execution completed successfully",
                            "response_chunks": response.response_chunks.len()
                        }))
                        .unwrap_or_default(),
                    };
                    let _ = state.sender.send(completion_message);
                }
                Err(e) => {
                    let mut state = session_state.lock().await;
                    state.update_execution_status(execution_id, ExecutionStatus::Failed);

                    let error_message = WebsocketMessage {
                        message_type: WebsocketMessageType::ExecutionError(execution_id),
                        data: serde_json::to_vec(&json!({
                            "status": "failed",
                            "error": e.to_string()
                        }))
                        .unwrap_or_default(),
                    };
                    let _ = state.sender.send(error_message);
                }
            }

            {
                let mut state = session_state.lock().await;
                state.remove_execution(execution_id);
            }
        }
        WebsocketMessageType::ExecutionComplete(_id) => {}
        WebsocketMessageType::ExecutionInput(_id) => {}
        WebsocketMessageType::ExecutionInterrupt(_id) => {}
        WebsocketMessageType::ExecutionUpdate(_id) => {}
        WebsocketMessageType::RequestContextResponse(_id) => {}
        _ => {}
    };
}
