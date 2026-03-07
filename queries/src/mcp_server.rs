use crate::prelude::*;
use models::{McpConnectionMetadata, McpServer, McpServerConfig};
use sqlx::Row;

pub struct GetMcpServersQuery {
    deployment_id: i64,
    limit: Option<u32>,
    offset: Option<u32>,
}

impl GetMcpServersQuery {
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

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> StdResult<Vec<McpServer>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let limit = self.limit.unwrap_or(50) as i64;
        let offset = self.offset.unwrap_or(0) as i64;

        let rows = sqlx::query(
            r#"
            SELECT id, created_at, updated_at, deployment_id, name, config
            FROM mcp_servers
            WHERE deployment_id = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(self.deployment_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut *conn)
        .await
        .map_err(AppError::Database)?;

        rows.into_iter()
            .map(|row| {
                let config_value: serde_json::Value = row.get("config");
                let config: McpServerConfig =
                    serde_json::from_value(config_value).map_err(|e| {
                        AppError::Internal(format!("Failed to decode MCP config: {}", e))
                    })?;

                Ok(McpServer {
                    id: row.get("id"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    deployment_id: row.get("deployment_id"),
                    name: row.get("name"),
                    config,
                })
            })
            .collect()
    }
}

pub struct GetMcpServerByIdQuery {
    deployment_id: i64,
    mcp_server_id: i64,
}

impl GetMcpServerByIdQuery {
    pub fn new(deployment_id: i64, mcp_server_id: i64) -> Self {
        Self {
            deployment_id,
            mcp_server_id,
        }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> StdResult<McpServer, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let row = sqlx::query(
            r#"
            SELECT id, created_at, updated_at, deployment_id, name, config
            FROM mcp_servers
            WHERE id = $1 AND deployment_id = $2
            "#,
        )
        .bind(self.mcp_server_id)
        .bind(self.deployment_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("MCP server not found".to_string()))?;

        let config_value: serde_json::Value = row.get("config");
        let config: McpServerConfig = serde_json::from_value(config_value)
            .map_err(|e| AppError::Internal(format!("Failed to decode MCP config: {}", e)))?;

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

pub struct GetAgentMcpServersQuery {
    deployment_id: i64,
    agent_id: i64,
}

impl GetAgentMcpServersQuery {
    pub fn new(deployment_id: i64, agent_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
        }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> StdResult<Vec<McpServer>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rows = sqlx::query(
            r#"
            SELECT m.id, m.created_at, m.updated_at, m.deployment_id, m.name, m.config
            FROM mcp_servers m
            JOIN ai_agent_mcp_servers ams ON ams.mcp_server_id = m.id
            WHERE m.deployment_id = $1 AND ams.agent_id = $2 AND ams.deployment_id = $1
            ORDER BY m.created_at DESC
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.agent_id)
        .fetch_all(&mut *conn)
        .await
        .map_err(AppError::Database)?;

        rows.into_iter()
            .map(|row| {
                let config_value: serde_json::Value = row.get("config");
                let config: McpServerConfig =
                    serde_json::from_value(config_value).map_err(|e| {
                        AppError::Internal(format!("Failed to decode MCP config: {}", e))
                    })?;

                Ok(McpServer {
                    id: row.get("id"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    deployment_id: row.get("deployment_id"),
                    name: row.get("name"),
                    config,
                })
            })
            .collect()
    }
}

pub struct GetActiveAgentMcpServerIdsForContextQuery {
    deployment_id: i64,
    agent_id: i64,
    context_group: String,
}

pub struct GetActiveAgentMcpServerConnectionMetadataQuery {
    deployment_id: i64,
    agent_id: i64,
    context_group: String,
    mcp_server_id: i64,
}

impl GetActiveAgentMcpServerConnectionMetadataQuery {
    pub fn new(
        deployment_id: i64,
        agent_id: i64,
        context_group: String,
        mcp_server_id: i64,
    ) -> Self {
        Self {
            deployment_id,
            agent_id,
            context_group,
            mcp_server_id,
        }
    }
}

impl GetActiveAgentMcpServerIdsForContextQuery {
    pub fn new(deployment_id: i64, agent_id: i64, context_group: String) -> Self {
        Self {
            deployment_id,
            agent_id,
            context_group,
        }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> StdResult<Vec<i64>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await.map_err(AppError::Database)?;
        let rows = sqlx::query(
            r#"
            SELECT mcp_server_id
            FROM active_agent_mcp_servers
            WHERE deployment_id = $1
              AND agent_id = $2
              AND context_group = $3
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.agent_id)
        .bind(&self.context_group)
        .fetch_all(&mut *conn)
        .await
        .map_err(AppError::Database)?;

        Ok(rows
            .into_iter()
            .map(|row| row.get::<i64, _>("mcp_server_id"))
            .collect())
    }
}

impl GetActiveAgentMcpServerConnectionMetadataQuery {
    pub async fn execute_with_db<'a, A>(
        &self,
        acquirer: A,
    ) -> StdResult<Option<McpConnectionMetadata>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await.map_err(AppError::Database)?;
        let row = sqlx::query(
            r#"
            SELECT connection_metadata
            FROM active_agent_mcp_servers
            WHERE deployment_id = $1
              AND agent_id = $2
              AND context_group = $3
              AND mcp_server_id = $4
            LIMIT 1
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.agent_id)
        .bind(&self.context_group)
        .bind(self.mcp_server_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(AppError::Database)?;

        let metadata = row
            .and_then(|r| {
                r.try_get::<Option<serde_json::Value>, _>("connection_metadata")
                    .ok()
                    .flatten()
            })
            .map(serde_json::from_value::<McpConnectionMetadata>)
            .transpose()
            .map_err(|e| {
                AppError::Internal(format!("Failed to decode MCP connection metadata: {}", e))
            })?;

        Ok(metadata)
    }
}
