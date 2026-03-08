use crate::dynamic_update_set::DynamicUpdateSet;
use chrono::Utc;
use common::{HasDbRouter, HasIdProvider, error::AppError};
use models::{AgentIntegration, IntegrationType};
use sqlx::Row;
use std::str::FromStr;

pub struct CreateAgentIntegrationCommand {
    id: Option<i64>,
    deployment_id: i64,
    agent_id: i64,
    integration_type: IntegrationType,
    name: String,
    config: serde_json::Value,
}

#[derive(Default)]
pub struct CreateAgentIntegrationCommandBuilder {
    id: Option<i64>,
    deployment_id: Option<i64>,
    agent_id: Option<i64>,
    integration_type: Option<IntegrationType>,
    name: Option<String>,
    config: Option<serde_json::Value>,
}

impl CreateAgentIntegrationCommand {
    pub fn builder() -> CreateAgentIntegrationCommandBuilder {
        CreateAgentIntegrationCommandBuilder::default()
    }

    pub fn new(
        deployment_id: i64,
        agent_id: i64,
        integration_type: IntegrationType,
        name: String,
        config: serde_json::Value,
    ) -> Self {
        Self {
            id: None,
            deployment_id,
            agent_id,
            integration_type,
            name,
            config,
        }
    }
}

impl CreateAgentIntegrationCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<AgentIntegration, AppError>
    where
        D: HasDbRouter + HasIdProvider,
    {
        let id = self.id.unwrap_or(deps.id_provider().next_id()? as i64);
        CreateAgentIntegrationCommand {
            id: Some(id),
            ..self
        }
        .execute_with_db(deps.db_router().writer())
        .await
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<AgentIntegration, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = self
            .id
            .ok_or_else(|| AppError::Validation("id is required".into()))?;
        let now = Utc::now();

        let integration = sqlx::query!(
            r#"
            INSERT INTO agent_integrations (id, created_at, updated_at, deployment_id, agent_id, integration_type, name, config)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, created_at, updated_at, deployment_id, agent_id, integration_type, name, config
            "#,
            id,
            now,
            now,
            self.deployment_id,
            self.agent_id,
            self.integration_type.to_string(),
            self.name,
            self.config,
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(AgentIntegration {
            id: integration.id,
            created_at: integration.created_at,
            updated_at: integration.updated_at,
            deployment_id: integration.deployment_id,
            agent_id: integration.agent_id,
            integration_type: self.integration_type,
            name: integration.name,
            config: integration.config,
        })
    }
}

impl CreateAgentIntegrationCommandBuilder {
    pub fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn agent_id(mut self, agent_id: i64) -> Self {
        self.agent_id = Some(agent_id);
        self
    }

    pub fn integration_type(mut self, integration_type: IntegrationType) -> Self {
        self.integration_type = Some(integration_type);
        self
    }

    pub fn name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn config(mut self, config: serde_json::Value) -> Self {
        self.config = Some(config);
        self
    }

    pub fn build(self) -> Result<CreateAgentIntegrationCommand, AppError> {
        Ok(CreateAgentIntegrationCommand {
            id: self.id,
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
            agent_id: self
                .agent_id
                .ok_or_else(|| AppError::Validation("agent_id is required".into()))?,
            integration_type: self
                .integration_type
                .ok_or_else(|| AppError::Validation("integration_type is required".into()))?,
            name: self
                .name
                .ok_or_else(|| AppError::Validation("name is required".into()))?,
            config: self
                .config
                .ok_or_else(|| AppError::Validation("config is required".into()))?,
        })
    }
}

pub struct UpdateAgentIntegrationCommand {
    deployment_id: i64,
    integration_id: i64,
    name: Option<String>,
    config: Option<serde_json::Value>,
}

#[derive(Default)]
pub struct UpdateAgentIntegrationCommandBuilder {
    deployment_id: Option<i64>,
    integration_id: Option<i64>,
    name: Option<String>,
    config: Option<serde_json::Value>,
}

impl UpdateAgentIntegrationCommand {
    pub fn builder() -> UpdateAgentIntegrationCommandBuilder {
        UpdateAgentIntegrationCommandBuilder::default()
    }

    pub fn new(deployment_id: i64, integration_id: i64) -> Self {
        Self {
            deployment_id,
            integration_id,
            name: None,
            config: None,
        }
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn with_config(mut self, config: serde_json::Value) -> Self {
        self.config = Some(config);
        self
    }
}

impl UpdateAgentIntegrationCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<AgentIntegration, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = Utc::now();

        let mut update_set = DynamicUpdateSet::with_updated_at();
        update_set.push_if_present("name", &self.name);
        update_set.push_if_present("config", &self.config);
        let (id_param, deployment_param) = update_set.where_indexes();

        let query = format!(
            r#"
            UPDATE agent_integrations
            SET {}
            WHERE id = ${} AND deployment_id = ${}
            RETURNING id, created_at, updated_at, deployment_id, agent_id, integration_type, name, config
            "#,
            update_set.set_clause(),
            id_param,
            deployment_param
        );

        let mut query_builder = sqlx::query(&query);
        query_builder = query_builder.bind(now);

        if let Some(name) = self.name {
            query_builder = query_builder.bind(name);
        }
        if let Some(config) = self.config {
            query_builder = query_builder.bind(config);
        }

        query_builder = query_builder
            .bind(self.integration_id)
            .bind(self.deployment_id);

        let row = query_builder
            .fetch_one(executor)
            .await
            .map_err(AppError::Database)?;

        let integration_type_str: String = row.get("integration_type");
        let integration_type =
            IntegrationType::from_str(&integration_type_str).map_err(AppError::BadRequest)?;

        Ok(AgentIntegration {
            id: row.get("id"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            deployment_id: row.get("deployment_id"),
            agent_id: row.get("agent_id"),
            integration_type,
            name: row.get("name"),
            config: row.get("config"),
        })
    }
}

impl UpdateAgentIntegrationCommandBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn integration_id(mut self, integration_id: i64) -> Self {
        self.integration_id = Some(integration_id);
        self
    }

    pub fn name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    pub fn config(mut self, config: Option<serde_json::Value>) -> Self {
        self.config = config;
        self
    }

    pub fn build(self) -> Result<UpdateAgentIntegrationCommand, AppError> {
        Ok(UpdateAgentIntegrationCommand {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
            integration_id: self
                .integration_id
                .ok_or_else(|| AppError::Validation("integration_id is required".into()))?,
            name: self.name,
            config: self.config,
        })
    }
}

pub struct DeleteAgentIntegrationCommand {
    deployment_id: i64,
    integration_id: i64,
}

#[derive(Default)]
pub struct DeleteAgentIntegrationCommandBuilder {
    deployment_id: Option<i64>,
    integration_id: Option<i64>,
}

impl DeleteAgentIntegrationCommand {
    pub fn builder() -> DeleteAgentIntegrationCommandBuilder {
        DeleteAgentIntegrationCommandBuilder::default()
    }

    pub fn new(deployment_id: i64, integration_id: i64) -> Self {
        Self {
            deployment_id,
            integration_id,
        }
    }
}

impl DeleteAgentIntegrationCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            "DELETE FROM agent_integrations WHERE id = $1 AND deployment_id = $2",
            self.integration_id,
            self.deployment_id
        )
        .execute(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(())
    }
}

impl DeleteAgentIntegrationCommandBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn integration_id(mut self, integration_id: i64) -> Self {
        self.integration_id = Some(integration_id);
        self
    }

    pub fn build(self) -> Result<DeleteAgentIntegrationCommand, AppError> {
        Ok(DeleteAgentIntegrationCommand {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
            integration_id: self
                .integration_id
                .ok_or_else(|| AppError::Validation("integration_id is required".into()))?,
        })
    }
}
