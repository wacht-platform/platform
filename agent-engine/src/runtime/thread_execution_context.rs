use std::sync::Arc;

use common::error::AppError;
use common::state::AppState;
use models::{
    default_embedding_dimension, is_supported_embedding_dimension, AgentThreadState,
    AiAgentWithFeatures, DeploymentAiSettings,
};
use queries::GetAgentThreadStateQuery;
use tokio::sync::RwLock;

use crate::llm::{GeminiClient, LlmRole, OpenAiClient, OpenRouterClient, ResolvedLlm};
use crate::runtime::vector_store::VectorStore;

#[derive(Debug, Clone, Default)]
pub struct DeploymentProviderKeys {
    pub strong_llm_provider: Option<String>,
    pub weak_llm_provider: Option<String>,
    pub gemini_api_key: Option<String>,
    pub openrouter_api_key: Option<String>,
    pub openrouter_require_parameters: bool,
    pub openai_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub strong_model: Option<String>,
    pub weak_model: Option<String>,
    pub embedding_dimension: i32,
}

impl DeploymentProviderKeys {
    pub fn from_settings(
        settings: Option<&DeploymentAiSettings>,
        encryption_service: &common::EncryptionService,
    ) -> Result<Self, AppError> {
        let embedding_dimension = settings
            .map(|s| s.embedding_dimension)
            .unwrap_or_else(default_embedding_dimension);
        if !is_supported_embedding_dimension(embedding_dimension) {
            return Err(AppError::Validation(format!(
                "Unsupported deployment embedding dimension: {}",
                embedding_dimension
            )));
        }

        Ok(Self {
            strong_llm_provider: settings.map(|s| s.strong_llm_provider.clone()),
            weak_llm_provider: settings.map(|s| s.weak_llm_provider.clone()),
            gemini_api_key: settings
                .and_then(|s| s.gemini_api_key.as_deref())
                .map(|value| encryption_service.decrypt(value))
                .transpose()?,
            openrouter_api_key: settings
                .and_then(|s| s.openrouter_api_key.as_deref())
                .map(|value| encryption_service.decrypt(value))
                .transpose()?,
            openrouter_require_parameters: settings
                .map(|s| s.openrouter_require_parameters)
                .unwrap_or(true),
            openai_api_key: settings
                .and_then(|s| s.openai_api_key.as_deref())
                .map(|value| encryption_service.decrypt(value))
                .transpose()?,
            anthropic_api_key: settings
                .and_then(|s| s.anthropic_api_key.as_deref())
                .map(|value| encryption_service.decrypt(value))
                .transpose()?,
            strong_model: settings.and_then(|s| s.strong_model.clone()),
            weak_model: settings.and_then(|s| s.weak_model.clone()),
            embedding_dimension,
        })
    }
}

pub struct ThreadExecutionContext {
    pub app_state: AppState,
    pub agent: AiAgentWithFeatures,
    pub thread_id: i64,
    pub actor_id: i64,
    pub execution_run_id: i64,
    pub provider_keys: DeploymentProviderKeys,
    pub vector_store: Arc<dyn VectorStore>,
    cached_thread: RwLock<Option<AgentThreadState>>,
}

impl ThreadExecutionContext {
    fn agent_override_for(&self, role: LlmRole) -> Option<&models::AgentModelOverride> {
        // Either role falls back to the OTHER agent override before deployment
        // defaults. Most callers set only one of strong/weak on the agent and
        // expect it to apply everywhere on that agent; only when BOTH are set
        // do the roles diverge.
        let candidate = match role {
            LlmRole::Strong => self
                .agent
                .strong_model
                .as_ref()
                .or(self.agent.weak_model.as_ref()),
            LlmRole::Weak => self
                .agent
                .weak_model
                .as_ref()
                .or(self.agent.strong_model.as_ref()),
        }?;
        if candidate.provider.trim().is_empty() || candidate.model.trim().is_empty() {
            return None;
        }
        Some(candidate)
    }

    fn llm_provider(&self, role: LlmRole) -> &str {
        if let Some(over) = self.agent_override_for(role) {
            return over.provider.as_str();
        }
        let provider = match role {
            LlmRole::Strong => self.provider_keys.strong_llm_provider.as_deref(),
            LlmRole::Weak => self.provider_keys.weak_llm_provider.as_deref(),
        };
        provider
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("gemini")
    }

    fn resolve_model_name(&self, role: LlmRole) -> &str {
        if let Some(over) = self.agent_override_for(role) {
            return over.model.as_str();
        }
        let deployment_default = match role {
            LlmRole::Strong => self.provider_keys.strong_model.as_deref(),
            LlmRole::Weak => self.provider_keys.weak_model.as_deref(),
        };
        if let Some(value) = deployment_default.filter(|v| !v.trim().is_empty()) {
            return value;
        }
        let provider = self.llm_provider(role);
        let fallback = match (role, provider) {
            (LlmRole::Strong, "openrouter") | (LlmRole::Weak, "openrouter") => {
                "nvidia/nemotron-3-super-120b-a12b:free"
            }
            (LlmRole::Strong, "openai") => "gpt-5.1",
            (LlmRole::Weak, "openai") => "gpt-5-mini",
            (LlmRole::Strong, _) => "gemini-3.1-pro-preview",
            (LlmRole::Weak, _) => "gemini-3-flash-preview",
        };
        tracing::warn!(
            deployment_id = self.agent.deployment_id,
            agent_id = self.agent.id,
            role = ?role,
            provider,
            fallback,
            "LLM model not configured (no agent override, no deployment default); using hardcoded fallback",
        );
        fallback
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_thread(
        app_state: AppState,
        agent: AiAgentWithFeatures,
        thread_id: i64,
        actor_id: i64,
        execution_run_id: i64,
        provider_keys: DeploymentProviderKeys,
        vector_store: Arc<dyn VectorStore>,
        cached_thread: Option<AgentThreadState>,
    ) -> Arc<Self> {
        Arc::new(Self {
            app_state,
            agent,
            thread_id,
            actor_id,
            execution_run_id,
            provider_keys,
            vector_store,
            cached_thread: RwLock::new(cached_thread),
        })
    }

    pub fn with_agent(self: &Arc<Self>, agent: AiAgentWithFeatures) -> Arc<Self> {
        let carried_thread = self
            .cached_thread
            .try_read()
            .ok()
            .and_then(|guard| guard.clone());
        Arc::new(Self {
            app_state: self.app_state.clone(),
            agent,
            thread_id: self.thread_id,
            actor_id: self.actor_id,
            execution_run_id: self.execution_run_id,
            provider_keys: self.provider_keys.clone(),
            vector_store: self.vector_store.clone(),
            cached_thread: RwLock::new(carried_thread),
        })
    }

    pub async fn get_thread(&self) -> Result<AgentThreadState, AppError> {
        {
            let cache = self.cached_thread.read().await;
            if let Some(thread) = cache.as_ref() {
                return Ok(thread.clone());
            }
        }

        let thread = GetAgentThreadStateQuery::new(self.thread_id, self.agent.deployment_id)
            .execute_with_db(self.app_state.db_router.writer())
            .await?;

        {
            let mut cache = self.cached_thread.write().await;
            *cache = Some(thread.clone());
        }

        Ok(thread)
    }

    /// Get the thread title (cached)
    pub async fn thread_title(&self) -> Result<String, AppError> {
        let thread = self.get_thread().await?;
        if thread.title.is_empty() {
            Ok(format!("Thread {}", self.thread_id))
        } else {
            Ok(thread.title)
        }
    }

    pub fn invalidate_cache(&self) {
        if let Ok(mut cache) = self.cached_thread.try_write() {
            *cache = None;
        }
    }

    pub async fn create_llm(&self, role: LlmRole) -> Result<ResolvedLlm, AppError> {
        let model = self.resolve_model_name(role);
        let provider = self.llm_provider(role);
        match provider {
            "openrouter" => {
                let client = OpenRouterClient::from_api_key(
                    self.provider_keys.openrouter_api_key.clone(),
                    model,
                    self.provider_keys.openrouter_require_parameters,
                )?
                .with_billing_context(
                    self.agent.deployment_id,
                    self.thread_id,
                    self.actor_id,
                    self.app_state.nats_client.clone(),
                );
                return Ok(ResolvedLlm::new(Arc::new(client), model));
            }
            "openai" => {
                let client = OpenAiClient::from_api_key(
                    self.provider_keys.openai_api_key.clone(),
                    model,
                )?
                .with_billing_context(
                    self.agent.deployment_id,
                    self.thread_id,
                    self.actor_id,
                    self.app_state.nats_client.clone(),
                );
                return Ok(ResolvedLlm::new(Arc::new(client), model));
            }
            _ => {}
        }

        let client = GeminiClient::from_api_key(
            self.provider_keys.gemini_api_key.clone(),
            model,
            self.agent.deployment_id,
            self.thread_id,
            self.actor_id,
            self.app_state.redis_client.clone(),
            self.app_state.nats_client.clone(),
        )?;
        Ok(ResolvedLlm::new(Arc::new(client), model))
    }

    pub async fn get_thread_by_id(&self, thread_id: i64) -> Result<AgentThreadState, AppError> {
        GetAgentThreadStateQuery::new(thread_id, self.agent.deployment_id)
            .execute_with_db(self.app_state.db_router.writer())
            .await
    }

}
