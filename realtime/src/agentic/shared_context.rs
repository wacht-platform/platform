use crate::agentic::context_engine_executor::ContextEngineExecutor;
use shared::models::AiAgentWithFeatures;
use shared::state::AppState;
use std::sync::Arc;

/// Shared context that can be passed to all executors
#[derive(Clone)]
pub struct SharedExecutionContext {
    pub app_state: AppState,
    pub context_engine: Arc<ContextEngineExecutor>,
    pub agent: AiAgentWithFeatures,
    pub context_id: i64,
}

impl SharedExecutionContext {
    pub fn new(
        app_state: AppState,
        context_id: i64,
        agent: AiAgentWithFeatures,
    ) -> Self {
        let context_engine = Arc::new(ContextEngineExecutor::new(
            app_state.clone(),
            context_id,
            agent.clone(),
        ));

        Self {
            app_state,
            context_engine,
            agent,
            context_id,
        }
    }

    /// Get a reference to the context engine
    pub fn context_engine(&self) -> &ContextEngineExecutor {
        &self.context_engine
    }

    /// Get the app state
    pub fn app_state(&self) -> &AppState {
        &self.app_state
    }

    /// Get the agent
    pub fn agent(&self) -> &AiAgentWithFeatures {
        &self.agent
    }

    /// Get the context ID
    pub fn context_id(&self) -> i64 {
        self.context_id
    }
}