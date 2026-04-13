use chrono::Utc;
use common::error::AppError;
use models::{McpServer, McpServerConfig};

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

        let config_json = self
            .config
            .map(|config| {
                serde_json::to_value(config).map_err(|e| AppError::Serialization(e.to_string()))
            })
            .transpose()?;

        let row = sqlx::query!(
            r#"
            UPDATE mcp_servers
            SET
                updated_at = $1,
                name = COALESCE($2, name),
                config = COALESCE($3, config)
            WHERE id = $4 AND deployment_id = $5
            RETURNING id, created_at, updated_at, deployment_id, name, config as "config!: serde_json::Value"
            "#,
            Utc::now(),
            self.name,
            config_json,
            self.mcp_server_id,
            self.deployment_id
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
