use axum::response::IntoResponse;
use fastwebsockets::OpCode;
use fastwebsockets::WebSocketError;
use fastwebsockets::upgrade;

pub async fn handler(ws: upgrade::IncomingUpgrade) -> impl IntoResponse {
    let (response, fut) = ws.upgrade().unwrap();

    tokio::task::spawn(async move {
        if let Err(e) = handle_client(fut).await {
            eprintln!("Error in websocket connection: {}", e);
        }
    });

    response
}

pub enum WebsocketMessageType {
    RequestContext(Option<u64>),
    RequestContextResponse(u64),
    ExecutionUpdate(u64),
    ExecutionComplete(u64),
    ExecutionInput(u64),
    ExecutionInterrupt(u64),
}

pub struct WebsocketMessage {
    pub message_type: WebsocketMessageType,
}

async fn handle_client(fut: upgrade::UpgradeFut) -> Result<(), WebSocketError> {
    let mut ws = fastwebsockets::FragmentCollector::new(fut.await?);

    loop {
        let frame = ws.read_frame().await?;
        match frame.opcode {
            OpCode::Close => break,
            OpCode::Text | OpCode::Binary => {
                ws.write_frame(frame).await?;
            }
            _ => {}
        }
    }

    Ok(())
}
