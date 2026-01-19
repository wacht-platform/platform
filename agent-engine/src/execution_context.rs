use std::sync::Arc;

use common::error::AppError;
use common::state::AppState;
use models::{AgentExecutionContext, AiAgentWithFeatures, DeploymentAiSettings};
use queries::{GetExecutionContextQuery, Query};
use tokio::sync::RwLock;

use crate::gemini::GeminiClient;

/// Cached integration status for the context
#[derive(Clone, Default)]
pub struct IntegrationStatus {
    pub teams_enabled: bool,
    pub clickup_enabled: bool,
}

pub struct ExecutionContext {
    pub app_state: AppState,
    pub agent: AiAgentWithFeatures,
    pub context_id: i64,
    pub deployment_ai_settings: Option<DeploymentAiSettings>,
    cached_context: RwLock<Option<AgentExecutionContext>>,
    cached_integration_status: RwLock<Option<IntegrationStatus>>,
}

impl ExecutionContext {
    pub fn new(
        app_state: AppState,
        agent: AiAgentWithFeatures,
        context_id: i64,
        deployment_ai_settings: Option<DeploymentAiSettings>,
    ) -> Arc<Self> {
        Arc::new(Self {
            app_state,
            agent,
            context_id,
            deployment_ai_settings,
            cached_context: RwLock::new(None),
            cached_integration_status: RwLock::new(None),
        })
    }

    pub async fn get_context(&self) -> Result<AgentExecutionContext, AppError> {
        {
            let cache = self.cached_context.read().await;
            if let Some(ctx) = cache.as_ref() {
                return Ok(ctx.clone());
            }
        }

        let ctx = GetExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
            .execute(&self.app_state)
            .await?;

        {
            let mut cache = self.cached_context.write().await;
            *cache = Some(ctx.clone());
        }

        Ok(ctx)
    }

    /// Get the context title (cached)
    pub async fn context_title(&self) -> Result<String, AppError> {
        let ctx = self.get_context().await?;
        if ctx.title.is_empty() {
            Ok(format!("Context {}", self.context_id))
        } else {
            Ok(ctx.title)
        }
    }

    /// Get integration status (teams_enabled, clickup_enabled) - computed once and cached
    pub async fn integration_status(&self) -> Result<IntegrationStatus, AppError> {
        {
            let cache = self.cached_integration_status.read().await;
            if let Some(status) = cache.as_ref() {
                return Ok(status.clone());
            }
        }

        let context = self.get_context().await?;
        let mut status = IntegrationStatus::default();

        if let Some(context_group) = &context.context_group {
            let active_integrations = queries::GetActiveIntegrationsForContextQuery::new(
                self.agent.deployment_id,
                self.agent.id,
                context_group.clone(),
            )
            .execute(&self.app_state)
            .await?;

            status.teams_enabled = active_integrations
                .iter()
                .any(|i| matches!(i.integration_type, models::IntegrationType::Teams));

            status.clickup_enabled = active_integrations
                .iter()
                .any(|i| matches!(i.integration_type, models::IntegrationType::ClickUp));

            if status.teams_enabled {
                tracing::info!(
                    "Context group {} has active Teams integration.",
                    context_group
                );
            }
            if status.clickup_enabled {
                tracing::info!(
                    "Context group {} has active ClickUp integration.",
                    context_group
                );
            }
        }

        {
            let mut cache = self.cached_integration_status.write().await;
            *cache = Some(status.clone());
        }

        Ok(status)
    }

    pub fn invalidate_cache(&self) {
        if let Ok(mut cache) = self.cached_context.try_write() {
            *cache = None;
        }
        if let Ok(mut cache) = self.cached_integration_status.try_write() {
            *cache = None;
        }
    }

    pub async fn create_llm(&self, model: &str) -> Result<GeminiClient, AppError> {
        let context = self.get_context().await?;
        GeminiClient::from_deployment(
            self.deployment_ai_settings.as_ref(),
            &self.app_state.encryption_service,
            model,
            self.agent.deployment_id,
            self.context_id,
            context.context_group,
            self.app_state.redis_client.clone(),
            self.app_state.nats_client.clone(),
        )
    }

    /// Get execution context for a different context_id (useful for cross-context operations)
    pub async fn get_context_by_id(&self, context_id: i64) -> Result<AgentExecutionContext, AppError> {
        GetExecutionContextQuery::new(context_id, self.agent.deployment_id)
            .execute(&self.app_state)
            .await
    }

    /// Get ClickUp client for the current context's group
    pub async fn get_clickup_client(&self) -> Result<crate::clickup::ClickUpClient, AppError> {
        let context = self.get_context().await?;
        let context_group = context.context_group.ok_or_else(|| {
            AppError::BadRequest("No context group found for ClickUp command".to_string())
        })?;

        let access_token = queries::GetClickUpTokenQuery::new(
            self.agent.deployment_id,
            context_group,
        )
        .execute(&self.app_state)
        .await?;

        Ok(crate::clickup::ClickUpClient::new(access_token))
    }
}
