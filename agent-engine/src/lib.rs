pub(crate) mod executor;
pub(crate) mod filesystem;
mod json_schema;
pub(crate) mod llm;
mod runtime;
pub(crate) mod sandbox;
pub mod tools;

// Stable public surface — consumed by the worker.
pub use runtime::{
    AgentHandler, ExecutionRequest, LanceDbVectorStore, LanceDbVectorStoreFactory, SecretsProvider,
    SettingsSecretsProvider, VectorStore, VectorStoreFactory,
};
pub use sandbox::init_shared_sandbox_runtime;

// Crate-internal re-exports for sibling modules that import via `crate::*`.
pub(crate) use executor::{AgentExecutor, ResumeContext};

use std::sync::OnceLock;

static CONSOLE_DEPLOYMENT_ID: OnceLock<i64> = OnceLock::new();

pub(crate) fn console_deployment_id() -> i64 {
    *CONSOLE_DEPLOYMENT_ID.get_or_init(|| match std::env::var("CONSOLE_DEPLOYMENT_ID") {
        Ok(value) => value.parse::<i64>().unwrap_or_else(|err| {
            tracing::warn!(value = %value, error = %err, "CONSOLE_DEPLOYMENT_ID unparseable; falling back to 0");
            0
        }),
        Err(_) => {
            tracing::warn!("CONSOLE_DEPLOYMENT_ID not set; falling back to 0");
            0
        }
    })
}
