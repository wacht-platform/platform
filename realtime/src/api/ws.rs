use std::sync::Arc;

use axum::extract::State;
use axum::response::IntoResponse;
use fastwebsockets::FragmentCollector;
use fastwebsockets::Frame;
use fastwebsockets::OpCode;
use fastwebsockets::WebSocketError;
use fastwebsockets::upgrade;
use serde_json::Value;
use serde_json::json;
use shared::models::AgentExecutionContextMessage;
use shared::queries::GetAiAgentByNameQuery;
use shared::queries::GetExecutionMessagesQuery;
use shared::state::AppState;
use tokio::sync::{Mutex, mpsc};

use super::models::{WebsocketMessage, WebsocketMessageType};
use super::session::SessionState;
use crate::api::agent::{AgentHandler, ExecutionRequest};
use shared::queries::{GetExecutionContextQuery, Query};

pub async fn realtime_agent_handler(
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
    let (sender, mut receiver) = mpsc::unbounded_channel::<WebsocketMessage<Value>>();
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
            let _ = match serde_json::from_slice::<WebsocketMessage<Value>>(&frame.payload) {
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
    message: WebsocketMessage<Value>,
    app_state: AppState,
    session_state: Arc<Mutex<SessionState>>,
) {
    let (deployment_id, sender) = {
        let state = session_state.lock().await;
        (state.deployment_id.clone(), state.sender.clone())
    };

    if let WebsocketMessageType::SessionConnect(context_id, agent_name) = message.message_type {
        let message = match GetExecutionContextQuery::new(context_id, deployment_id)
            .execute(&app_state)
            .await
        {
            Ok(context) => WebsocketMessage {
                message_id: message.message_id,
                message_type: WebsocketMessageType::SessionConnected,
                data: json!(context),
            },
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

        match GetAiAgentByNameQuery::new(deployment_id, agent_name)
            .execute(&app_state)
            .await
        {
            Ok(agent) => {
                let mut session = session_state.lock().await;
                session.agent = Some(agent);

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
        (state.context_id.unwrap(), state.agent.clone().unwrap())
    };

    match message.message_type {
        WebsocketMessageType::FetchContextMessages => {
            let message = match GetExecutionMessagesQuery::new(context_id)
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
                    message_type: WebsocketMessageType::CloseConnection,
                    data: json!(Vec::<AgentExecutionContextMessage>::new()),
                },
            };

            let _ = sender.send(message);
        }
        WebsocketMessageType::MessageInput(user_input) => {
            let execution_request = ExecutionRequest {
                agent,
                deployment_id,
                user_message: user_input.clone(),
                context_id,
            };

            let execution_result = AgentHandler::new(app_state)
                .execute_agent_streaming(execution_request, move |chunk| {
                    dbg!(chunk);
                })
                .await;

            match execution_result {
                Ok(response) => {
                    dbg!(response);
                }
                Err(e) => {
                    eprintln!("{e}")
                }
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
