use crate::dynamic_update_set::DynamicUpdateSet;
use chrono::Utc;
use common::error::AppError;
use models::{McpServer, McpServerConfig};
use sqlx::Row;

pub struct CreateMcpServerCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub name: String,
    pub config: McpServerConfig,
}

impl CreateMcpServerCommand {
    pub fn new(id: i64, deployment_id: i64, name: String, config: McpServerConfig) -> Self {
        Self {
            id,
            deployment_id,
            name,
            config,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<McpServer, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if self.name.trim().is_empty() {
            return Err(AppError::BadRequest(
                "MCP server name is required".to_string(),
            ));
        }

        let now = Utc::now();
        let config_json = serde_json::to_value(&self.config)
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        let row = sqlx::query!(
            r#"
            INSERT INTO mcp_servers (id, created_at, updated_at, deployment_id, name, config)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, created_at, updated_at, deployment_id, name, config as "config!: serde_json::Value"
            "#,
            self.id,
            now,
            now,
            self.deployment_id,
            self.name,
            config_json
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        let config: McpServerConfig = serde_json::from_value(row.config)
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        Ok(McpServer {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deployment_id: row.deployment_id,
            name: row.name,
            config,
        })
    }
}

pub struct UpdateMcpServerCommand {
    pub deployment_id: i64,
    pub mcp_server_id: i64,
    pub name: Option<String>,
    pub config: Option<McpServerConfig>,
}

impl UpdateMcpServerCommand {
    pub fn new(deployment_id: i64, mcp_server_id: i64) -> Self {
        Self {
            deployment_id,
            mcp_server_id,
            name: None,
            config: None,
        }
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn with_config(mut self, config: McpServerConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<McpServer, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if let Some(name) = &self.name {
            if name.trim().is_empty() {
                return Err(AppError::BadRequest(
                    "MCP server name cannot be empty".to_string(),
                ));
            }
        }

        let mut update_set = DynamicUpdateSet::with_updated_at();
        update_set.push_if_present("name", &self.name);
        update_set.push_if_present("config", &self.config);
        let (id_param, deployment_param) = update_set.where_indexes();

        let query = format!(
            r#"
            UPDATE mcp_servers
            SET {}
            WHERE id = ${} AND deployment_id = ${}
            RETURNING id, created_at, updated_at, deployment_id, name, config
            "#,
            update_set.set_clause(),
            id_param,
            deployment_param,
        );

        let mut builder = sqlx::query(&query).bind(Utc::now());
        if let Some(name) = self.name {
            builder = builder.bind(name);
        }
        if let Some(config) = self.config {
            let config_json = serde_json::to_value(&config)
                .map_err(|e| AppError::Serialization(e.to_string()))?;
            builder = builder.bind(config_json);
        }
        builder = builder.bind(self.mcp_server_id).bind(self.deployment_id);

        let row = builder
            .fetch_one(executor)
            .await
            .map_err(AppError::Database)?;

        let config_value: serde_json::Value = row.get("config");
        let config: McpServerConfig = serde_json::from_value(config_value)
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        Ok(McpServer {
            id: row.get("id"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            deployment_id: row.get("deployment_id"),
            name: row.get("name"),
            config,
        })
    }
}

pub struct DeleteMcpServerCommand {
    pub deployment_id: i64,
    pub mcp_server_id: i64,
}

impl DeleteMcpServerCommand {
    pub fn new(deployment_id: i64, mcp_server_id: i64) -> Self {
        Self {
            deployment_id,
            mcp_server_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            "DELETE FROM mcp_servers WHERE id = $1 AND deployment_id = $2",
            self.mcp_server_id,
            self.deployment_id
        )
        .execute(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(())
    }
}

pub struct AttachMcpServerToAgentCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub mcp_server_id: i64,
}

impl AttachMcpServerToAgentCommand {
    pub fn new(deployment_id: i64, agent_id: i64, mcp_server_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
            mcp_server_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            WITH
                agent AS (
                    SELECT id
                    FROM ai_agents
                    WHERE id = $2 AND deployment_id = $1
                ),
                server AS (
                    SELECT id
                    FROM mcp_servers
                    WHERE id = $3 AND deployment_id = $1
                ),
                ins AS (
                    INSERT INTO ai_agent_mcp_servers (deployment_id, agent_id, mcp_server_id)
                    SELECT $1, agent.id, server.id
                    FROM agent
                    CROSS JOIN server
                    ON CONFLICT DO NOTHING
                    RETURNING 1
                )
            SELECT
                EXISTS(SELECT 1 FROM agent) AS "agent_exists!",
                EXISTS(SELECT 1 FROM server) AS "server_exists!",
                EXISTS(SELECT 1 FROM ins) AS "inserted!"
            "#,
            self.deployment_id,
            self.agent_id,
            self.mcp_server_id
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        if !result.agent_exists {
            return Err(AppError::NotFound("Agent not found".to_string()));
        }

        if !result.server_exists {
            return Err(AppError::NotFound("MCP server not found".to_string()));
        }

        Ok(())
    }
}

pub struct DetachMcpServerFromAgentCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub mcp_server_id: i64,
}

impl DetachMcpServerFromAgentCommand {
    pub fn new(deployment_id: i64, agent_id: i64, mcp_server_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
            mcp_server_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            DELETE FROM ai_agent_mcp_servers ams
            WHERE ams.agent_id = $1
              AND ams.mcp_server_id = $2
              AND ams.deployment_id = $3
              AND EXISTS (
                SELECT 1
                FROM ai_agents a
                JOIN mcp_servers m ON m.id = ams.mcp_server_id
                WHERE a.id = ams.agent_id
                  AND a.deployment_id = $3
                  AND m.deployment_id = $3
              )
            "#,
            self.agent_id,
            self.mcp_server_id,
            self.deployment_id
        )
        .execute(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(())
    }
}
