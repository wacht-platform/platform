use crate::Command;
use chrono::Utc;
use common::error::AppError;
use common::state::AppState;
use models::{AgentIntegration, IntegrationType};
use sqlx::Row;

pub struct CreateAgentIntegrationCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub integration_type: IntegrationType,
    pub name: String,
    pub config: serde_json::Value,
}

impl CreateAgentIntegrationCommand {
    pub fn new(
        deployment_id: i64,
        agent_id: i64,
        integration_type: IntegrationType,
        name: String,
        config: serde_json::Value,
    ) -> Self {
        Self {
            deployment_id,
            agent_id,
            integration_type,
            name,
            config,
        }
    }
}

impl Command for CreateAgentIntegrationCommand {
    type Output = AgentIntegration;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let id = app_state.sf.next_id()? as i64;
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
        .fetch_one(&app_state.db_pool)
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

pub struct UpdateAgentIntegrationCommand {
    pub deployment_id: i64,
    pub integration_id: i64,
    pub name: Option<String>,
    pub config: Option<serde_json::Value>,
}

impl UpdateAgentIntegrationCommand {
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

impl Command for UpdateAgentIntegrationCommand {
    type Output = AgentIntegration;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();

        let mut query_parts = vec!["updated_at = $1".to_string()];
        let mut param_count = 2;

        if self.name.is_some() {
            query_parts.push(format!("name = ${}", param_count));
            param_count += 1;
        }
        if self.config.is_some() {
            query_parts.push(format!("config = ${}", param_count));
            param_count += 1;
        }

        let query = format!(
            r#"
            UPDATE agent_integrations
            SET {}
            WHERE id = ${} AND deployment_id = ${}
            RETURNING id, created_at, updated_at, deployment_id, agent_id, integration_type, name, config
            "#,
            query_parts.join(", "),
            param_count,
            param_count + 1
        );

        let mut query_builder = sqlx::query(&query);
        query_builder = query_builder.bind(now);

        if let Some(name) = self.name {
            query_builder = query_builder.bind(name);
        }
        if let Some(config) = self.config {
            query_builder = query_builder.bind(config);
        }

        query_builder = query_builder.bind(self.integration_id).bind(self.deployment_id);

        let row = query_builder
            .fetch_one(&app_state.db_pool)
            .await
            .map_err(AppError::Database)?;

        let integration_type_str: String = row.get("integration_type");
        let integration_type = match integration_type_str.as_str() {
            "teams" => IntegrationType::Teams,
            "slack" => IntegrationType::Slack,
            "whatsapp" => IntegrationType::WhatsApp,
            "discord" => IntegrationType::Discord,
            _ => return Err(AppError::BadRequest("Unknown integration type".to_string())),
        };

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

pub struct DeleteAgentIntegrationCommand {
    pub deployment_id: i64,
    pub integration_id: i64,
}

impl DeleteAgentIntegrationCommand {
    pub fn new(deployment_id: i64, integration_id: i64) -> Self {
        Self {
            deployment_id,
            integration_id,
        }
    }
}

impl Command for DeleteAgentIntegrationCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!(
            "DELETE FROM agent_integrations WHERE id = $1 AND deployment_id = $2",
            self.integration_id,
            self.deployment_id
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(AppError::Database)?;

        Ok(())
    }
}
