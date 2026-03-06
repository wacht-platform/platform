use crate::prelude::*;
use models::{AgentIntegration, IntegrationType};
use std::str::FromStr;

pub struct GetAgentIntegrationsQuery {
    deployment_id: i64,
    agent_id: i64,
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Default)]
pub struct GetAgentIntegrationsQueryBuilder {
    deployment_id: Option<i64>,
    agent_id: Option<i64>,
    limit: Option<u32>,
    offset: Option<u32>,
}

impl GetAgentIntegrationsQuery {
    pub fn builder() -> GetAgentIntegrationsQueryBuilder {
        GetAgentIntegrationsQueryBuilder::default()
    }

    pub fn new(deployment_id: i64, agent_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
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

    pub async fn execute_with<'a, A>(
        &self,
        acquirer: A,
    ) -> StdResult<Vec<AgentIntegration>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let limit = self.limit.unwrap_or(50) as i64;
        let offset = self.offset.unwrap_or(0) as i64;

        let rows = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deployment_id, agent_id, integration_type, name, config
            FROM agent_integrations
            WHERE deployment_id = $1 AND agent_id = $2
            ORDER BY created_at DESC
            LIMIT $3 OFFSET $4
            "#,
            self.deployment_id,
            self.agent_id,
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await
        .map_err(AppError::Database)?;

        Ok(rows
            .into_iter()
            .map(|r| AgentIntegration {
                id: r.id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deployment_id: r.deployment_id,
                agent_id: r.agent_id,
                integration_type: parse_integration_type(&r.integration_type),
                name: r.name,
                config: r.config,
            })
            .collect())
    }
}

impl GetAgentIntegrationsQueryBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn agent_id(mut self, agent_id: i64) -> Self {
        self.agent_id = Some(agent_id);
        self
    }

    pub fn limit(mut self, limit: Option<u32>) -> Self {
        self.limit = limit;
        self
    }

    pub fn offset(mut self, offset: Option<u32>) -> Self {
        self.offset = offset;
        self
    }

    pub fn build(self) -> StdResult<GetAgentIntegrationsQuery, AppError> {
        Ok(GetAgentIntegrationsQuery {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
            agent_id: self
                .agent_id
                .ok_or_else(|| AppError::Validation("agent_id is required".into()))?,
            limit: self.limit,
            offset: self.offset,
        })
    }
}

fn parse_integration_type(s: &str) -> IntegrationType {
    match IntegrationType::from_str(s) {
        Ok(kind) => kind,
        Err(_) => IntegrationType::Teams,
    }
}

impl Query for GetAgentIntegrationsQuery {
    type Output = Vec<AgentIntegration>;

    async fn execute(&self, app_state: &AppState) -> StdResult<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}

pub struct GetAgentIntegrationByIdQuery {
    deployment_id: i64,
    agent_id: i64,
    integration_id: i64,
}

#[derive(Default)]
pub struct GetAgentIntegrationByIdQueryBuilder {
    deployment_id: Option<i64>,
    agent_id: Option<i64>,
    integration_id: Option<i64>,
}

impl GetAgentIntegrationByIdQuery {
    pub fn builder() -> GetAgentIntegrationByIdQueryBuilder {
        GetAgentIntegrationByIdQueryBuilder::default()
    }

    pub fn new(deployment_id: i64, agent_id: i64, integration_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
            integration_id,
        }
    }

    pub async fn execute_with<'a, A>(
        &self,
        acquirer: A,
    ) -> StdResult<AgentIntegration, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let row = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deployment_id, agent_id, integration_type, name, config
            FROM agent_integrations
            WHERE id = $1 AND deployment_id = $2 AND agent_id = $3
            "#,
            self.integration_id,
            self.deployment_id,
            self.agent_id,
        )
        .fetch_optional(&mut *conn)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("Integration not found".to_string()))?;

        Ok(AgentIntegration {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deployment_id: row.deployment_id,
            agent_id: row.agent_id,
            integration_type: parse_integration_type(&row.integration_type),
            name: row.name,
            config: row.config,
        })
    }
}

impl GetAgentIntegrationByIdQueryBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn agent_id(mut self, agent_id: i64) -> Self {
        self.agent_id = Some(agent_id);
        self
    }

    pub fn integration_id(mut self, integration_id: i64) -> Self {
        self.integration_id = Some(integration_id);
        self
    }

    pub fn build(self) -> StdResult<GetAgentIntegrationByIdQuery, AppError> {
        Ok(GetAgentIntegrationByIdQuery {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
            agent_id: self
                .agent_id
                .ok_or_else(|| AppError::Validation("agent_id is required".into()))?,
            integration_id: self
                .integration_id
                .ok_or_else(|| AppError::Validation("integration_id is required".into()))?,
        })
    }
}

impl Query for GetAgentIntegrationByIdQuery {
    type Output = AgentIntegration;

    async fn execute(&self, app_state: &AppState) -> StdResult<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
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

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> StdResult<Vec<AgentIntegration>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await.map_err(AppError::Database)?;
        let rows = sqlx::query!(
            r#"
            SELECT i.id, i.created_at, i.updated_at, i.deployment_id, i.agent_id, i.integration_type, i.name, i.config
            FROM active_agent_integrations aai
            JOIN agent_integrations i ON i.id = aai.integration_id
            WHERE aai.context_group = $1
              AND aai.deployment_id = $2
              AND aai.agent_id = $3
            "#,
            self.context_group,
            self.deployment_id,
            self.agent_id,
        )
        .fetch_all(&mut *conn)
        .await
        .map_err(AppError::Database)?;

        Ok(rows
            .into_iter()
            .map(|r| AgentIntegration {
                id: r.id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deployment_id: r.deployment_id,
                agent_id: r.agent_id,
                integration_type: parse_integration_type(&r.integration_type),
                name: r.name,
                config: r.config,
            })
            .collect())
    }
}

impl Query for GetActiveIntegrationsForContextQuery {
    type Output = Vec<AgentIntegration>;

    async fn execute(&self, app_state: &AppState) -> StdResult<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}

pub struct GetClickUpTokenQuery {
    deployment_id: i64,
    context_group: String,
}

impl GetClickUpTokenQuery {
    pub fn new(deployment_id: i64, context_group: String) -> Self {
        Self {
            deployment_id,
            context_group,
        }
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> StdResult<String, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await.map_err(AppError::Database)?;
        let row: Option<(Option<serde_json::Value>,)> = sqlx::query_as(
            r#"
            SELECT aai.connection_metadata
            FROM active_agent_integrations aai
            JOIN agent_integrations i ON i.id = aai.integration_id
            WHERE aai.deployment_id = $1
              AND aai.context_group = $2
              AND i.integration_type = 'clickup'
            LIMIT 1
            "#,
        )
        .bind(self.deployment_id)
        .bind(&self.context_group)
        .fetch_optional(&mut *conn)
        .await
        .map_err(AppError::Database)?;

        let metadata = row
            .ok_or_else(|| AppError::NotFound("No active ClickUp integration found".to_string()))?
            .0
            .ok_or_else(|| {
                AppError::NotFound("ClickUp connection metadata is missing".to_string())
            })?;

        let access_token = metadata
            .get("accessToken")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::NotFound("ClickUp access token not found in metadata".to_string())
            })?;

        Ok(access_token.to_string())
    }
}

impl Query for GetClickUpTokenQuery {
    type Output = String;

    async fn execute(&self, app_state: &AppState) -> StdResult<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}
