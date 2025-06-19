use axum::extract::State;
use axum::response::IntoResponse;
use fastwebsockets::FragmentCollector;
use fastwebsockets::Frame;
use fastwebsockets::OpCode;
use fastwebsockets::WebSocketError;
use fastwebsockets::upgrade;
use shared::state::AppState;

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

#[derive(serde::Serialize, serde::Deserialize)]
pub enum WebsocketMessageType {
    #[serde(rename = "request_context")]
    RequestContext(Option<u64>),
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
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct WebsocketMessage {
    pub message_type: WebsocketMessageType,
    pub data: Vec<u8>,
}

async fn handle_client(fut: upgrade::UpgradeFut, state: AppState) -> Result<(), WebSocketError> {
    let mut ws = FragmentCollector::new(fut.await?);
    let (sender, receiver) = kanal::bounded_async::<WebsocketMessage>(8);

    loop {
        tokio::select! {
            to_continue = async {
                let frame = ws.read_frame().await.unwrap();
                match frame.opcode {
                    OpCode::Close => false,
                    OpCode::Text | OpCode::Binary => {
                        let state = state.clone();
                        let sender = sender.clone();

                        tokio::spawn(async move {
                            match serde_json::from_slice::<WebsocketMessage>(&frame.payload) {
                                Ok(message) => handle_message(message, state, sender).await,
                                Err(_) => (),
                            }
                        });

                        true
                    }
                    _ => true
                }
            } => {
                if !to_continue {
                    break;
                }
            },
            Ok(message) = receiver.recv() => {
                let payload = serde_json::to_vec(&message).unwrap();
                ws.write_frame(Frame::binary(fastwebsockets::Payload::Owned(payload))).await;
            }
        }
    }

    Ok(())
}

async fn handle_message(
    message: WebsocketMessage,
    state: AppState,
    sender: kanal::AsyncSender<WebsocketMessage>,
) {
    match message.message_type {
        WebsocketMessageType::RequestContext(Some(id)) => {}
        WebsocketMessageType::ExecutionComplete(id) => {}
        WebsocketMessageType::ExecutionInput(id) => {}
        WebsocketMessageType::ExecutionInterrupt(id) => {}
        WebsocketMessageType::ExecutionUpdate(id) => {}
        WebsocketMessageType::RequestContextResponse(id) => {}
        _ => {}
    }
}
