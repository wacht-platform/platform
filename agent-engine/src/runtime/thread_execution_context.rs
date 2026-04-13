use std::sync::Arc;

use commands::ResolveDeploymentStorageCommand;
use common::error::AppError;
use common::state::AppState;
use common::{
    connect_vector_store, open_knowledge_base_table_in_connection, open_memory_table_in_connection,
};
use lancedb::{Connection, Table};
use models::{AgentThreadState, AiAgentWithFeatures, DeploymentAiSettings};
use queries::GetAgentThreadStateQuery;
use tokio::sync::RwLock;

use crate::llm::{GeminiClient, LlmClient, LlmRole, OpenAiClient, OpenRouterClient, ResolvedLlm};

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
}

impl DeploymentProviderKeys {
    pub fn from_settings(
        settings: Option<&DeploymentAiSettings>,
        encryption_service: &common::EncryptionService,
    ) -> Result<Self, AppError> {
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
        })
    }
}

pub struct ThreadExecutionContext {
    pub app_state: AppState,
    pub agent: AiAgentWithFeatures,
    pub thread_id: i64,
    pub execution_run_id: i64,
    pub provider_keys: DeploymentProviderKeys,
    cached_thread: RwLock<Option<AgentThreadState>>,
    cached_kb_connection: RwLock<Option<Connection>>,
    cached_kb_table: RwLock<Option<Table>>,
    cached_memory_table: RwLock<Option<Table>>,
}

impl ThreadExecutionContext {
    fn llm_provider(&self, role: LlmRole) -> &str {
        let provider = match role {
            LlmRole::Strong => self.provider_keys.strong_llm_provider.as_deref(),
            LlmRole::Weak => self.provider_keys.weak_llm_provider.as_deref(),
        };
        provider
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("gemini")
    }

    fn resolve_model_name(&self, role: LlmRole) -> &str {
        match role {
            LlmRole::Strong => self
                .provider_keys
                .strong_model
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| match self.llm_provider(role) {
                    "openrouter" => "nvidia/nemotron-3-super-120b-a12b:free",
                    "openai" => "gpt-5.1",
                    _ => "gemini-3.1-pro-preview",
                }),
            LlmRole::Weak => self
                .provider_keys
                .weak_model
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| match self.llm_provider(role) {
                    "openrouter" => "nvidia/nemotron-3-super-120b-a12b:free",
                    "openai" => "gpt-5-mini",
                    _ => "gemini-3-flash-preview",
                }),
        }
    }

    pub fn new(
        app_state: AppState,
        agent: AiAgentWithFeatures,
        thread_id: i64,
        execution_run_id: i64,
        provider_keys: DeploymentProviderKeys,
    ) -> Arc<Self> {
        Arc::new(Self {
            app_state,
            agent,
            thread_id,
            execution_run_id,
            provider_keys,
            cached_thread: RwLock::new(None),
            cached_kb_connection: RwLock::new(None),
            cached_kb_table: RwLock::new(None),
            cached_memory_table: RwLock::new(None),
        })
    }

    /// Create a new ThreadExecutionContext with a replaced agent.
    pub fn with_agent(self: &Arc<Self>, agent: AiAgentWithFeatures) -> Arc<Self> {
        Arc::new(Self {
            app_state: self.app_state.clone(),
            agent,
            thread_id: self.thread_id,
            execution_run_id: self.execution_run_id,
            provider_keys: self.provider_keys.clone(),
            cached_thread: RwLock::new(None),
            cached_kb_connection: RwLock::new(None),
            cached_kb_table: RwLock::new(None),
            cached_memory_table: RwLock::new(None),
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
        if self.llm_provider(role) == "openrouter" {
            let client = OpenRouterClient::from_api_key(
                self.provider_keys.openrouter_api_key.clone(),
                model,
                self.provider_keys.openrouter_require_parameters,
            )?;
            return Ok(ResolvedLlm::new(LlmClient::OpenRouter(client), model));
        }

        if self.llm_provider(role) == "openai" {
            let client =
                OpenAiClient::from_api_key(self.provider_keys.openai_api_key.clone(), model)?;
            return Ok(ResolvedLlm::new(LlmClient::OpenAi(client), model));
        }

        let client = GeminiClient::from_api_key(
            self.provider_keys.gemini_api_key.clone(),
            model,
            self.agent.deployment_id,
            self.thread_id,
            self.app_state.redis_client.clone(),
            self.app_state.nats_client.clone(),
        )?;
        Ok(ResolvedLlm::new(LlmClient::Gemini(client), model))
    }

    pub async fn get_thread_by_id(&self, thread_id: i64) -> Result<AgentThreadState, AppError> {
        GetAgentThreadStateQuery::new(thread_id, self.agent.deployment_id)
            .execute_with_db(self.app_state.db_router.writer())
            .await
    }

    pub async fn get_kb_connection(&self) -> Result<Connection, AppError> {
        {
            let cache = self.cached_kb_connection.read().await;
            if let Some(conn) = cache.as_ref() {
                return Ok(conn.clone());
            }
        }

        let storage = ResolveDeploymentStorageCommand::new(self.agent.deployment_id)
            .execute_with_deps(&common::deps::from_app(&self.app_state).db().enc())
            .await?;
        let config = storage.vector_store_config();

        let conn = connect_vector_store(&config).await?;

        {
            let mut cache = self.cached_kb_connection.write().await;
            *cache = Some(conn.clone());
        }

        Ok(conn)
    }

    pub async fn get_kb_table(&self) -> Result<Option<Table>, AppError> {
        {
            let cache = self.cached_kb_table.read().await;
            if let Some(table) = cache.as_ref() {
                return Ok(Some(table.clone()));
            }
        }

        let conn = self.get_kb_connection().await?;
        let table = open_knowledge_base_table_in_connection(&conn).await?;

        if let Some(table) = table.as_ref() {
            let mut cache = self.cached_kb_table.write().await;
            *cache = Some(table.clone());
        }

        Ok(table)
    }

    pub async fn get_memory_table(&self) -> Result<Option<Table>, AppError> {
        {
            let cache = self.cached_memory_table.read().await;
            if let Some(table) = cache.as_ref() {
                return Ok(Some(table.clone()));
            }
        }

        let conn = self.get_kb_connection().await?;
        let table = open_memory_table_in_connection(&conn).await?;

        if let Some(table) = table.as_ref() {
            let mut cache = self.cached_memory_table.write().await;
            *cache = Some(table.clone());
        }

        Ok(table)
    }
}
