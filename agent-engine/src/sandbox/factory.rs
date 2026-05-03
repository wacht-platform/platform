use std::sync::{Arc, OnceLock};

use super::{NatsSandboxRuntime, SandboxRuntime};
use wacht_sandbox_client::SandboxNatsClient;

#[derive(Clone)]
pub struct SandboxRuntimeFactory {
    default_runtime: Arc<dyn SandboxRuntime>,
}

impl SandboxRuntimeFactory {
    pub fn new(default_runtime: Arc<dyn SandboxRuntime>) -> Self {
        Self { default_runtime }
    }

    pub fn for_deployment(&self, _deployment_id: &str) -> Arc<dyn SandboxRuntime> {
        self.default_runtime.clone()
    }
}

static SHARED_FACTORY: OnceLock<Arc<SandboxRuntimeFactory>> = OnceLock::new();

pub async fn init_shared_sandbox_runtime(
    nats: async_nats::Client,
) -> Result<(), String> {
    let client = SandboxNatsClient::new(nats);
    client.warm().await.map_err(|e| e.to_string())?;
    let runtime = Arc::new(NatsSandboxRuntime::new(client));
    let factory = Arc::new(SandboxRuntimeFactory::new(runtime));
    SHARED_FACTORY
        .set(factory)
        .map_err(|_| "sandbox runtime already initialized".to_string())
}

pub fn shared_sandbox_runtime() -> Arc<SandboxRuntimeFactory> {
    SHARED_FACTORY
        .get()
        .cloned()
        .expect("sandbox runtime not initialized; call init_shared_sandbox_runtime first")
}
