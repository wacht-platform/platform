use chrono::Utc;
use common::error::AppError;
use models::{McpServer, McpServerConfig, mcp_server::slugify_mcp_server_name};
use sqlx::types::Json;

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

        let slug = slugify_mcp_server_name(&self.name);
        if slug.is_empty() {
            return Err(AppError::BadRequest(
                "MCP server name must contain at least one alphanumeric character".to_string(),
            ));
        }

        let now = Utc::now();
        let config_json = Json(self.config);

        let row = sqlx::query!(
            r#"
            INSERT INTO mcp_servers (id, created_at, updated_at, deployment_id, name, slug, config)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, created_at, updated_at, deployment_id, name, slug, config as "config!: Json<McpServerConfig>"
            "#,
            self.id,
            now,
            now,
            self.deployment_id,
            self.name,
            slug,
            config_json as _
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(McpServer {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deployment_id: row.deployment_id,
            name: row.name,
            slug: row.slug,
            config: row.config.0,
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

        let config_json = self.config.map(Json);

        let row = sqlx::query!(
            r#"
            UPDATE mcp_servers
            SET
                updated_at = $1,
                name = COALESCE($2, name),
                config = COALESCE($3, config)
            WHERE id = $4 AND deployment_id = $5
            RETURNING id, created_at, updated_at, deployment_id, name, slug, config as "config!: Json<McpServerConfig>"
            "#,
            Utc::now(),
            self.name,
            config_json as _,
            self.mcp_server_id,
            self.deployment_id
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(McpServer {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deployment_id: row.deployment_id,
            name: row.name,
            slug: row.slug,
            config: row.config.0,
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

pub struct CreateMcpOAuthStateCommand {
    pub state: String,
    pub deployment_id: i64,
    pub actor_id: i64,
    pub mcp_server_id: i64,
    pub code_verifier: String,
    pub client_id: String,
    pub token_url: String,
    pub redirect_uri: String,
    pub resource: Option<String>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl CreateMcpOAuthStateCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            INSERT INTO mcp_oauth_states (
                state, deployment_id, actor_id, mcp_server_id, code_verifier,
                client_id, token_url, redirect_uri, resource, expires_at,
                created_at, updated_at
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,NOW(),NOW())
            "#,
            self.state,
            self.deployment_id,
            self.actor_id,
            self.mcp_server_id,
            self.code_verifier,
            self.client_id,
            self.token_url,
            self.redirect_uri,
            self.resource,
            self.expires_at,
        )
        .execute(executor)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }
}

pub struct DeleteActorMcpConnectionCommand {
    pub deployment_id: i64,
    pub actor_id: i64,
    pub mcp_server_id: i64,
}

impl DeleteActorMcpConnectionCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            DELETE FROM actor_mcp_server_connections
            WHERE deployment_id = $1 AND actor_id = $2 AND mcp_server_id = $3
            "#,
            self.deployment_id,
            self.actor_id,
            self.mcp_server_id,
        )
        .execute(executor)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }
}
