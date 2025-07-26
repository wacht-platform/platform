use shared::models::AiAgentWithFeatures;
use shared::state::AppState;

/// Shared context that can be passed to all executors
#[derive(Clone)]
pub struct SharedExecutionContext {
    pub app_state: AppState,
    pub agent: AiAgentWithFeatures,
    pub context_id: i64,
}

impl SharedExecutionContext {
    pub fn new(
        app_state: AppState,
        context_id: i64,
        agent: AiAgentWithFeatures,
    ) -> Self {
        Self {
            app_state,
            agent,
            context_id,
        }
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