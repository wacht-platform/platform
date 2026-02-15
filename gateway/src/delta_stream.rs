// Delta stream for syncing rate limit state across gateway instances
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RateLimitDelta {
    pub key: String,
    pub gateway_id: String,
    pub delta: u32,
    pub timestamp: i64,
}

#[derive(Clone)]
pub struct DeltaPublisher {
    tx: broadcast::Sender<RateLimitDelta>,
}

impl DeltaPublisher {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1000); // Buffer 1000 deltas
        Self { tx }
    }

    pub fn publish(&self, delta: RateLimitDelta) {
        // Non-blocking send - if no receivers, delta is dropped
        let _ = self.tx.send(delta);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RateLimitDelta> {
        self.tx.subscribe()
    }
}

pub struct DeltaConsumer {
    rx: broadcast::Receiver<RateLimitDelta>,
}

impl DeltaConsumer {
    pub fn new(publisher: &DeltaPublisher) -> Self {
        Self {
            rx: publisher.subscribe(),
        }
    }

    pub async fn recv(&mut self) -> Option<RateLimitDelta> {
        match self.rx.recv().await {
            Ok(delta) => Some(delta),
            Err(_) => None,
        }
    }

    /// Try to receive without blocking
    pub fn try_recv(&mut self) -> Option<RateLimitDelta> {
        match self.rx.try_recv() {
            Ok(delta) => Some(delta),
            Err(_) => None,
        }
    }
}
