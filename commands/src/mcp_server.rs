use crate::Command;
use chrono::Utc;
use common::error::AppError;
use common::state::AppState;
use models::{McpServer, McpServerConfig};
use sqlx::Row;

pub struct CreateMcpServerCommand {
    pub deployment_id: i64,
    pub name: String,
    pub config: McpServerConfig,
}

impl CreateMcpServerCommand {
    pub fn new(deployment_id: i64, name: String, config: McpServerConfig) -> Self {
        Self {
            deployment_id,
            name,
            config,
        }
    }
}

impl Command for CreateMcpServerCommand {
    type Output = McpServer;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        if self.name.trim().is_empty() {
            return Err(AppError::BadRequest(
                "MCP server name is required".to_string(),
            ));
        }

        let id = app_state.sf.next_id()? as i64;
        let now = Utc::now();
        let config_json = serde_json::to_value(&self.config)
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        let row = sqlx::query(
            r#"
            INSERT INTO mcp_servers (id, created_at, updated_at, deployment_id, name, config)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, created_at, updated_at, deployment_id, name, config
            "#,
        )
        .bind(id)
        .bind(now)
        .bind(now)
        .bind(self.deployment_id)
        .bind(self.name)
        .bind(config_json)
        .fetch_one(&app_state.db_pool)
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
}

impl Command for UpdateMcpServerCommand {
    type Output = McpServer;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        if let Some(name) = &self.name {
            if name.trim().is_empty() {
                return Err(AppError::BadRequest(
                    "MCP server name cannot be empty".to_string(),
                ));
            }
        }

        let mut set_parts = vec!["updated_at = $1".to_string()];
        let mut param_count = 2;

        if self.name.is_some() {
            set_parts.push(format!("name = ${}", param_count));
            param_count += 1;
        }
        if self.config.is_some() {
            set_parts.push(format!("config = ${}", param_count));
            param_count += 1;
        }

        let query = format!(
            r#"
            UPDATE mcp_servers
            SET {}
            WHERE id = ${} AND deployment_id = ${}
            RETURNING id, created_at, updated_at, deployment_id, name, config
            "#,
            set_parts.join(", "),
            param_count,
            param_count + 1,
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
            .fetch_one(&app_state.db_pool)
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
}

impl Command for DeleteMcpServerCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query("DELETE FROM mcp_servers WHERE id = $1 AND deployment_id = $2")
            .bind(self.mcp_server_id)
            .bind(self.deployment_id)
            .execute(&app_state.db_pool)
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
}

impl Command for AttachMcpServerToAgentCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let agent_exists =
            sqlx::query("SELECT 1 FROM ai_agents WHERE id = $1 AND deployment_id = $2 LIMIT 1")
                .bind(self.agent_id)
                .bind(self.deployment_id)
                .fetch_optional(&app_state.db_pool)
                .await
                .map_err(AppError::Database)?
                .is_some();

        if !agent_exists {
            return Err(AppError::NotFound("Agent not found".to_string()));
        }

        let server_exists =
            sqlx::query("SELECT 1 FROM mcp_servers WHERE id = $1 AND deployment_id = $2 LIMIT 1")
                .bind(self.mcp_server_id)
                .bind(self.deployment_id)
                .fetch_optional(&app_state.db_pool)
                .await
                .map_err(AppError::Database)?
                .is_some();

        if !server_exists {
            return Err(AppError::NotFound("MCP server not found".to_string()));
        }

        sqlx::query(
            r#"
            INSERT INTO ai_agent_mcp_servers (deployment_id, agent_id, mcp_server_id)
            VALUES ($1, $2, $3)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.agent_id)
        .bind(self.mcp_server_id)
        .execute(&app_state.db_pool)
        .await
        .map_err(AppError::Database)?;

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
}

impl Command for DetachMcpServerFromAgentCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query(
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
        )
        .bind(self.agent_id)
        .bind(self.mcp_server_id)
        .bind(self.deployment_id)
        .execute(&app_state.db_pool)
        .await
        .map_err(AppError::Database)?;

        Ok(())
    }
}
