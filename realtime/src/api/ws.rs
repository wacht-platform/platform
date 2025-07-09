use async_nats::jetstream;
use axum::extract::State;
use axum::response::IntoResponse;
use fastwebsockets::FragmentCollector;
use fastwebsockets::Frame;
use fastwebsockets::OpCode;
use fastwebsockets::WebSocketError;
use fastwebsockets::upgrade;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use shared::models::AgentExecutionContextMessage;
use shared::queries::GetAiAgentByNameWithFeatures;
use shared::queries::GetExecutionMessagesQuery;
use shared::state::AppState;
use std::sync::Arc;
use tokio::sync::Notify;
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
    let session = Arc::new(Mutex::new(SessionState::new(
        sender.clone(),
        app_state.clone(),
        20220525523509059,
    )));
    let channel_ready = Arc::new(Notify::new());

    tokio::spawn({
        let session = session.clone();
        let channel_ready = channel_ready.clone();

        async move {
            let kv = app_state
                .nats_jetstream
                .create_key_value(jetstream::kv::Config {
                    bucket: "agent_execution_kv".to_string(),
                    ..Default::default()
                })
                .await
                .unwrap();

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
                session.context_id.clone().unwrap()
            };

            let context_msg_key = format!("{}", context_id);

            let consumer_stream = app_state
                .nats_jetstream
                .get_or_create_stream(jetstream::stream::Config {
                    name: "agent_execution_stream".to_string(),
                    subjects: vec!["agent_execution_stream.>".to_string()],
                    ..Default::default()
                })
                .await
                .unwrap();

            let mut active_msg = String::from_utf8(match kv.get(context_msg_key.clone()).await {
                Ok(Some(key)) => key.to_vec(),
                _ => vec![],
            })
            .unwrap();
            let mut watch = kv.watch(context_msg_key).await.unwrap();

            loop {
                tokio::select! {
                    _ = async {
                        let consumer = consumer_stream
                            .create_consumer(jetstream::consumer::pull::Config {
                                durable_name: Some(format!("{}", app_state.sf.next_id().unwrap())),
                                filter_subject: format!("agent_execution_stream.msg:{}", active_msg),
                                ..Default::default()
                            })
                            .await
                            .unwrap();

                       let mut stream = consumer.messages().await.unwrap().take(100);
                        while let Some(Ok(message)) = stream.next().await {
                            let chunk = String::from_utf8(message.payload.to_vec()).unwrap();
                            let _ = sender.send(WebsocketMessage {
                                message_id: None,
                                message_type: WebsocketMessageType::NewMessageChunk,
                                data: json!({
                                    "chunk": chunk
                                }),
                            });
                            let _ = message.ack().await;
                        }
                    } => {}
                    Some(Ok(entry)) = watch.next() => {
                        active_msg = String::from_utf8(entry.value.to_vec()).unwrap();
                        print!("{active_msg}");
                    }
                    _ =  close.notified() => {
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
                if let Err(e) = ws.write_frame(Frame::binary(fastwebsockets::Payload::Owned(payload))).await {
                    eprintln!("Error writing frame: {}", e);
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
            let _ = match serde_json::from_slice::<WebsocketMessage<Value>>(&frame.payload) {
                Ok(message) => {
                    tokio::spawn(handle_execution_message(message, session_state));
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
    session_state: Arc<Mutex<SessionState>>,
) {
    let (deployment_id, sender, app_state) = {
        let state = session_state.lock().await;
        (
            state.deployment_id.clone(),
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
                Ok(context) => WebsocketMessage {
                    message_id: message.message_id.clone(),
                    message_type: WebsocketMessageType::SessionConnected,
                    data: json!(context),
                },
                Err(e) => WebsocketMessage {
                    message_id: message.message_id.clone(),
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

                session.ready.notify_waiters();

                let _ = sender.send(message);
            }
            Err(e) => {
                let message = WebsocketMessage {
                    message_id: message.message_id.clone(),
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
                message_id: message.message_id.clone(),
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
            let message = match GetExecutionMessagesQuery::new(context_id)
                .execute(&app_state)
                .await
            {
                Ok(messages) => WebsocketMessage {
                    message_id: message.message_id.clone(),
                    message_type: WebsocketMessageType::FetchContextMessages,
                    data: json!(messages),
                },
                Err(_) => WebsocketMessage {
                    message_id: message.message_id.clone(),
                    message_type: WebsocketMessageType::CloseConnection,
                    data: json!(Vec::<AgentExecutionContextMessage>::new()),
                },
            };

            let _ = sender.send(message);
        }
        WebsocketMessageType::MessageInput(user_message) => {
            let execution_request = ExecutionRequest {
                agent,
                deployment_id,
                user_message,
                context_id,
            };

            let _ = AgentHandler::new(app_state)
                .execute_agent_streaming(execution_request)
                .await;
        }
        _ => {}
    };
}
