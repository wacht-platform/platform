use crate::{commands::Command, error::AppError, models::AiAgent, state::AppState};
use chrono::Utc;
use sqlx::Row;

pub struct CreateAiAgentCommand {
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub configuration: serde_json::Value,
}

impl CreateAiAgentCommand {
    pub fn new(
        deployment_id: i64,
        name: String,
        description: Option<String>,
        configuration: serde_json::Value,
    ) -> Self {
        Self {
            deployment_id,
            name,
            description,
            configuration,
        }
    }
}

impl Command for CreateAiAgentCommand {
    type Output = AiAgent;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let agent_id = app_state.sf.next_id()? as i64;
        let now = Utc::now();

        let agent = sqlx::query!(
            r#"
            INSERT INTO ai_agents (id, created_at, updated_at, name, description, deployment_id, configuration)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, created_at, updated_at, name, description, deployment_id, configuration
            "#,
            agent_id,
            now,
            now,
            self.name,
            self.description,
            self.deployment_id,
            self.configuration,
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(AiAgent {
            id: agent.id,
            created_at: agent.created_at,
            updated_at: agent.updated_at,
            name: agent.name,
            description: agent.description,
            deployment_id: agent.deployment_id,
            configuration: agent.configuration,
        })
    }
}

pub struct UpdateAiAgentCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub configuration: Option<serde_json::Value>,
}

impl UpdateAiAgentCommand {
    pub fn new(deployment_id: i64, agent_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
            name: None,
            description: None,
            configuration: None,
        }
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }

    pub fn with_configuration(mut self, configuration: serde_json::Value) -> Self {
        self.configuration = Some(configuration);
        self
    }
}

impl Command for UpdateAiAgentCommand {
    type Output = AiAgent;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();

        // Build dynamic query based on provided fields
        let mut query_parts = vec!["updated_at = $1".to_string()];
        let mut param_count = 2;

        if self.name.is_some() {
            query_parts.push(format!("name = ${}", param_count));
            param_count += 1;
        }
        if self.description.is_some() {
            query_parts.push(format!("description = ${}", param_count));
            param_count += 1;
        }
        if self.configuration.is_some() {
            query_parts.push(format!("configuration = ${}", param_count));
            param_count += 1;
        }

        let query = format!(
            r#"
            UPDATE ai_agents
            SET {}
            WHERE id = ${} AND deployment_id = ${}
            RETURNING id, created_at, updated_at, name, description, deployment_id, configuration
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
        if let Some(description) = self.description {
            query_builder = query_builder.bind(description);
        }
        if let Some(configuration) = self.configuration {
            query_builder = query_builder.bind(configuration);
        }

        query_builder = query_builder.bind(self.agent_id).bind(self.deployment_id);

        let agent = query_builder
            .fetch_one(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

        Ok(AiAgent {
            id: agent.get("id"),
            created_at: agent.get("created_at"),
            updated_at: agent.get("updated_at"),
            name: agent.get("name"),
            description: agent.get("description"),
            deployment_id: agent.get("deployment_id"),
            configuration: agent.get("configuration"),
        })
    }
}

pub struct DeleteAiAgentCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
}

impl DeleteAiAgentCommand {
    pub fn new(deployment_id: i64, agent_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
        }
    }
}

impl Command for DeleteAiAgentCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut tx = app_state
            .db_pool
            .begin()
            .await
            .map_err(|e| AppError::Database(e))?;

        // Delete all agent relationships first
        sqlx::query!(
            "DELETE FROM ai_agent_tools WHERE agent_id = $1",
            self.agent_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        sqlx::query!(
            "DELETE FROM ai_agent_workflows WHERE agent_id = $1",
            self.agent_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        sqlx::query!(
            "DELETE FROM ai_agent_knowledge_bases WHERE agent_id = $1",
            self.agent_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Delete the agent
        sqlx::query!(
            "DELETE FROM ai_agents WHERE id = $1 AND deployment_id = $2",
            self.agent_id,
            self.deployment_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        tx.commit().await.map_err(|e| AppError::Database(e))?;

        Ok(())
    }
}
