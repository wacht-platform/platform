use crate::prelude::*;
use models::{McpConnectionMetadata, McpServer, McpServerConfig};

fn decode_mcp_config(config: serde_json::Value) -> StdResult<McpServerConfig, AppError> {
    serde_json::from_value(config)
        .map_err(|e| AppError::Internal(format!("Failed to decode MCP config: {}", e)))
}

fn build_mcp_server(
    id: i64,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deployment_id: i64,
    name: String,
    config: serde_json::Value,
) -> StdResult<McpServer, AppError> {
    Ok(McpServer {
        id,
        created_at,
        updated_at,
        deployment_id,
        name,
        config: decode_mcp_config(config)?,
    })
}

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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> StdResult<Vec<McpServer>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let limit = self.limit.unwrap_or(50) as i64;
        let offset = self.offset.unwrap_or(0) as i64;

        let rows = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deployment_id, name, config as "config!: serde_json::Value"
            FROM mcp_servers
            WHERE deployment_id = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
            self.deployment_id,
            limit,
            offset
        )
        .fetch_all(executor)
        .await
        .map_err(AppError::Database)?;

        rows.into_iter()
            .map(|row| {
                build_mcp_server(
                    row.id,
                    row.created_at,
                    row.updated_at,
                    row.deployment_id,
                    row.name,
                    row.config,
                )
            })
            .collect()
    }
}

pub struct ActorMcpConnection {
    pub server: McpServer,
    pub connection_metadata: Option<McpConnectionMetadata>,
}

pub struct GetActorMcpConnectionsQuery {
    deployment_id: i64,
    actor_id: i64,
}

impl GetActorMcpConnectionsQuery {
    pub fn new(deployment_id: i64, actor_id: i64) -> Self {
        Self {
            deployment_id,
            actor_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> StdResult<Vec<ActorMcpConnection>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            r#"
            SELECT
                ms.id,
                ms.created_at,
                ms.updated_at,
                ms.deployment_id,
                ms.name,
                ms.config as "config!: serde_json::Value",
                amsc.connection_metadata as "connection_metadata?: serde_json::Value"
            FROM mcp_servers ms
            LEFT JOIN actor_mcp_server_connections amsc
                ON amsc.mcp_server_id = ms.id
                AND amsc.deployment_id = $1
                AND amsc.actor_id = $2
            WHERE ms.deployment_id = $1
            ORDER BY ms.created_at DESC
            "#,
            self.deployment_id,
            self.actor_id,
        )
        .fetch_all(executor)
        .await
        .map_err(AppError::Database)?;

        rows.into_iter()
            .map(|row| {
                let server = build_mcp_server(
                    row.id,
                    row.created_at,
                    row.updated_at,
                    row.deployment_id,
                    row.name,
                    row.config,
                )?;
                let connection_metadata = row
                    .connection_metadata
                    .and_then(|m| serde_json::from_value::<McpConnectionMetadata>(m).ok());
                Ok(ActorMcpConnection {
                    server,
                    connection_metadata,
                })
            })
            .collect()
    }
}

pub struct UpdateActorMcpConnectionMetadataQuery {
    deployment_id: i64,
    actor_id: i64,
    mcp_server_id: i64,
    metadata: serde_json::Value,
}

impl UpdateActorMcpConnectionMetadataQuery {
    pub fn new(deployment_id: i64, actor_id: i64, mcp_server_id: i64, metadata: serde_json::Value) -> Self {
        Self { deployment_id, actor_id, mcp_server_id, metadata }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> StdResult<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE actor_mcp_server_connections
            SET connection_metadata = $4, updated_at = NOW()
            WHERE deployment_id = $1 AND actor_id = $2 AND mcp_server_id = $3
            "#,
            self.deployment_id,
            self.actor_id,
            self.mcp_server_id,
            self.metadata,
        )
        .execute(executor)
        .await
        .map_err(AppError::Database)?;
        Ok(())
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> StdResult<McpServer, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deployment_id, name, config as "config!: serde_json::Value"
            FROM mcp_servers
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.mcp_server_id,
            self.deployment_id
        )
        .fetch_optional(executor)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("MCP server not found".to_string()))?;

        build_mcp_server(
            row.id,
            row.created_at,
            row.updated_at,
            row.deployment_id,
            row.name,
            row.config,
        )
    }
}
