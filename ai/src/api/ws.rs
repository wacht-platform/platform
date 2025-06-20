use core::str;
use std::sync::Arc;

use axum::extract::State;
use axum::response::IntoResponse;
use fastwebsockets::FragmentCollector;
use fastwebsockets::Frame;
use fastwebsockets::OpCode;
use fastwebsockets::WebSocketError;
use fastwebsockets::upgrade;
use llm::builder::LLMBackend;
use llm::builder::LLMBuilder;
use shared::models::AiAgent;
use shared::queries::GetAiAgentByIdQuery;
use shared::state::AppState;
use tokio::sync::Mutex;

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

#[derive(serde::Serialize, serde::Deserialize, PartialEq)]
pub enum WebsocketMessageType {
    #[serde(rename = "request_context")]
    RequestContext(Option<u64>, String),
    #[serde(rename = "request_context_response")]
    RequestContextResponse(u64),
    #[serde(rename = "execution_update")]
    ExecutionUpdate(u64),
    #[serde(rename = "execution_complete")]
    ExecutionComplete(u64),
    #[serde(rename = "execution_input")]
    ExecutionInput(u64),
    #[serde(rename = "execution_interrupt")]
    ExecutionInterrupt(u64),
    #[serde(rename = "close_connection")]
    CloseConnection,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct WebsocketMessage {
    pub message_type: WebsocketMessageType,
    pub data: Vec<u8>,
}

pub struct SessionState {
    pub authenticated: bool,
    pub agent: Option<AiAgent>,
    pub sender: kanal::AsyncSender<WebsocketMessage>,
    pub deployment_id: i64,
}

impl SessionState {
    pub fn new(sender: kanal::AsyncSender<WebsocketMessage>, deployment_id: i64) -> Self {
        Self {
            authenticated: false,
            agent: None,
            sender,
            deployment_id,
        }
    }
}

async fn handle_client(
    fut: upgrade::UpgradeFut,
    app_state: AppState,
) -> Result<(), WebSocketError> {
    let mut ws = FragmentCollector::new(fut.await?);
    let (sender, receiver) = kanal::bounded_async::<WebsocketMessage>(8);
    let session_state = Arc::new(Mutex::new(SessionState::new(sender, 0)));

    loop {
        tokio::select! {
            Ok(frame) = ws.read_frame() => {
                let close = handler_websocket_message(frame, app_state.clone(), session_state.clone());
                if close {
                    break;
                }
            },
            Ok(message) = receiver.recv() => {
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

pub enum AgenticExecutionSender {
    User,
    Agent,
    System,
}

pub struct AgenticExecutionMessage {
    sender: AgenticExecutionSender,
    data: Vec<u8>,
}

pub enum AgenticExecutionState {
    Idle,
    Running,
    Interrupted,
    Completed,
}

pub struct AgenticExecutionContext {
    messages: Vec<AgenticExecutionMessage>,
    lstm: Vec<String>,
    current_goal: String,
    state: AgenticExecutionState,
    title: String,
    deployment_id: i64,
    authenticated: bool,
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
    use crate::agentic::AgentExecutor;

    match message.message_type {
        WebsocketMessageType::RequestContext(Some(_id), agent_name) => {
            let deployment_id = {
                let state = session_state.lock().await;
                state.deployment_id
            };

            // Parse the message data to get the user input
            let user_input = match String::from_utf8(message.data) {
                Ok(input) => input,
                Err(_) => {
                    eprintln!("Failed to parse user input");
                    return;
                }
            };

            // Create agent executor
            match AgentExecutor::new(&agent_name, deployment_id, &app_state).await {
                Ok(agent_executor) => {
                    let sender = {
                        let state = session_state.lock().await;
                        state.sender.clone()
                    };

                    // Execute with streaming
                    let _ = agent_executor.execute_with_streaming(&user_input, |chunk| {
                        let response_message = WebsocketMessage {
                            message_type: WebsocketMessageType::ExecutionUpdate(0),
                            data: chunk.as_bytes().to_vec(),
                        };

                        // Send chunk to client
                        let _ = sender.try_send(response_message);
                    }).await;

                    // Send completion message
                    let completion_message = WebsocketMessage {
                        message_type: WebsocketMessageType::ExecutionComplete(0),
                        data: Vec::new(),
                    };
                    let _ = sender.try_send(completion_message);
                }
                Err(e) => {
                    eprintln!("Failed to create agent executor: {}", e);

                    let error_message = WebsocketMessage {
                        message_type: WebsocketMessageType::ExecutionComplete(0),
                        data: format!("Error: {}", e).as_bytes().to_vec(),
                    };

                    let state = session_state.lock().await;
                    let _ = state.sender.try_send(error_message);
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
