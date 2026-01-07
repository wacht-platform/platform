use crate::prelude::*;
use models::{AgentIntegration, IntegrationType};

pub struct GetAgentIntegrationsQuery {
    deployment_id: i64,
    limit: Option<u32>,
    offset: Option<u32>,
}

impl GetAgentIntegrationsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            limit: None,
            offset: None,
        }
    }

    pub fn with_limit(mut self, limit: Option<u32>) -> Self {
        self.limit = limit;
        self
    }

    pub fn with_offset(mut self, offset: Option<u32>) -> Self {
        self.offset = offset;
        self
    }
}

fn parse_integration_type(s: &str) -> IntegrationType {
    match s {
        "teams" => IntegrationType::Teams,
        "slack" => IntegrationType::Slack,
        "whatsapp" => IntegrationType::WhatsApp,
        "discord" => IntegrationType::Discord,
        _ => IntegrationType::Teams,
    }
}

impl Query for GetAgentIntegrationsQuery {
    type Output = Vec<AgentIntegration>;

    async fn execute(&self, app_state: &AppState) -> StdResult<Self::Output, AppError> {
        let limit = self.limit.unwrap_or(50) as i64;
        let offset = self.offset.unwrap_or(0) as i64;

        let rows = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deployment_id, integration_type, name, config
            FROM agent_integrations
            WHERE deployment_id = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
            self.deployment_id,
            limit,
            offset,
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows
            .into_iter()
            .map(|r| AgentIntegration {
                id: r.id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deployment_id: r.deployment_id,
                integration_type: parse_integration_type(&r.integration_type),
                name: r.name,
                config: r.config,
            })
            .collect())
    }
}

pub struct GetAgentIntegrationByIdQuery {
    deployment_id: i64,
    integration_id: i64,
}

impl GetAgentIntegrationByIdQuery {
    pub fn new(deployment_id: i64, integration_id: i64) -> Self {
        Self {
            deployment_id,
            integration_id,
        }
    }
}

impl Query for GetAgentIntegrationByIdQuery {
    type Output = AgentIntegration;

    async fn execute(&self, app_state: &AppState) -> StdResult<Self::Output, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deployment_id, integration_type, name, config
            FROM agent_integrations
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.integration_id,
            self.deployment_id,
        )
        .fetch_optional(&app_state.db_pool)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("Integration not found".to_string()))?;

        Ok(AgentIntegration {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deployment_id: row.deployment_id,
            integration_type: parse_integration_type(&row.integration_type),
            name: row.name,
            config: row.config,
        })
    }
}

/// Query to get integrations attached to a specific agent via its integration_ids configuration
pub struct GetIntegrationsByAgentIdQuery {
    deployment_id: i64,
    agent_id: i64,
}

impl GetIntegrationsByAgentIdQuery {
    pub fn new(deployment_id: i64, agent_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
        }
    }
}

impl Query for GetIntegrationsByAgentIdQuery {
    type Output = Vec<AgentIntegration>;

    async fn execute(&self, app_state: &AppState) -> StdResult<Self::Output, AppError> {
        // First get agent's integration_ids from configuration
        let rows = sqlx::query!(
            r#"
            SELECT i.id, i.created_at, i.updated_at, i.deployment_id, i.integration_type, i.name, i.config
            FROM agent_integrations i
            INNER JOIN ai_agents a ON a.deployment_id = i.deployment_id
            WHERE a.id = $1 
                AND a.deployment_id = $2
                AND jsonb_typeof(a.configuration->'integration_ids') = 'array'
                AND i.id IN (SELECT value::bigint FROM jsonb_array_elements_text(a.configuration->'integration_ids'))
            "#,
            self.agent_id,
            self.deployment_id,
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows
            .into_iter()
            .map(|r| AgentIntegration {
                id: r.id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deployment_id: r.deployment_id,
                integration_type: parse_integration_type(&r.integration_type),
                name: r.name,
                config: r.config,
            })
            .collect())
    }
}

pub struct GetActiveIntegrationsForContextQuery {
    deployment_id: i64,
    agent_id: i64,
    context_group: String,
}

impl GetActiveIntegrationsForContextQuery {
    pub fn new(deployment_id: i64, agent_id: i64, context_group: String) -> Self {
        Self {
            deployment_id,
            agent_id,
            context_group,
        }
    }
}

impl Query for GetActiveIntegrationsForContextQuery {
    type Output = Vec<AgentIntegration>;

    async fn execute(&self, app_state: &AppState) -> StdResult<Self::Output, AppError> {
        let rows = sqlx::query!(
            r#"
            SELECT i.id, i.created_at, i.updated_at, i.deployment_id, i.integration_type, i.name, i.config
            FROM active_agent_integrations aai
            JOIN agent_integrations i ON i.id = aai.integration_id
            JOIN ai_agents a ON a.id = $3
            WHERE aai.context_group = $1
              AND aai.deployment_id = $2
              AND jsonb_typeof(a.configuration->'integration_ids') = 'array'
              AND i.id IN (SELECT value::bigint FROM jsonb_array_elements_text(a.configuration->'integration_ids'))
            "#,
            self.context_group,
            self.deployment_id,
            self.agent_id,
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows
            .into_iter()
            .map(|r| AgentIntegration {
                id: r.id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deployment_id: r.deployment_id,
                integration_type: parse_integration_type(&r.integration_type),
                name: r.name,
                config: r.config,
            })
            .collect())
    }
}
