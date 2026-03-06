use sqlx::Row;

use crate::Query;
use common::error::AppError;
use common::state::AppState;
use models::{AiTool, AiToolConfiguration, AiToolType, AiToolWithDetails};

pub struct GetAiToolsQuery {
    pub deployment_id: i64,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub search: Option<String>,
}

impl GetAiToolsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            limit: None,
            offset: None,
            search: None,
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

    pub fn with_search(mut self, search: Option<String>) -> Self {
        self.search = search;
        self
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<Vec<AiToolWithDetails>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let mut query = r#"
            SELECT
                t.id, t.created_at, t.updated_at, t.name, t.description,
                t.tool_type, t.deployment_id, t.configuration,
                COALESCE(a.agents_count, 0) as agents_count
            FROM ai_tools t
            LEFT JOIN (
                SELECT deployment_id, tool_id, COUNT(*) as agents_count
                FROM ai_agent_tools
                GROUP BY deployment_id, tool_id
            ) a ON t.id = a.tool_id AND t.deployment_id = a.deployment_id
            WHERE t.deployment_id = $1
        "#
        .to_string();

        let mut param_count = 2;
        if self.search.is_some() {
            query.push_str(&format!(
                " AND (t.name ILIKE ${} OR t.description ILIKE ${})",
                param_count,
                param_count + 1
            ));
            param_count += 2;
        }

        query.push_str(" ORDER BY t.created_at DESC");
        query.push_str(&format!(
            " LIMIT ${} OFFSET ${}",
            param_count,
            param_count + 1
        ));

        let mut query_builder = sqlx::query(&query);
        query_builder = query_builder.bind(self.deployment_id);

        if let Some(search) = &self.search {
            let search_pattern = format!("%{}%", search);
            query_builder = query_builder
                .bind(search_pattern.clone())
                .bind(search_pattern);
        }

        query_builder = query_builder
            .bind(self.limit.unwrap_or(50) as i64)
            .bind(self.offset.unwrap_or(0) as i64);

        let tools = query_builder
            .fetch_all(&mut *conn)
            .await
            .map_err(AppError::Database)?;

        Ok(tools
            .into_iter()
            .map(|row| {
                let tool_type = AiToolType::from(row.get::<String, _>("tool_type"));
                let configuration: AiToolConfiguration =
                    serde_json::from_value(row.get("configuration")).unwrap_or_default();

                AiToolWithDetails {
                    id: row.get("id"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    name: row.get("name"),
                    description: row.get("description"),
                    tool_type,
                    deployment_id: row.get("deployment_id"),
                    configuration,
                }
            })
            .collect())
    }
}

impl Query for GetAiToolsQuery {
    type Output = Vec<AiToolWithDetails>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}

pub struct GetAiToolByIdQuery {
    pub deployment_id: i64,
    pub tool_id: i64,
}

impl GetAiToolByIdQuery {
    pub fn new(deployment_id: i64, tool_id: i64) -> Self {
        Self {
            deployment_id,
            tool_id,
        }
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<AiToolWithDetails, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let tool = sqlx::query!(
            r#"
            SELECT
                t.id, t.created_at, t.updated_at, t.name, t.description,
                t.tool_type, t.deployment_id, t.configuration,
                COALESCE(a.agents_count, 0) as agents_count
            FROM ai_tools t
            LEFT JOIN (
                SELECT deployment_id, tool_id, COUNT(*) as agents_count
                FROM ai_agent_tools
                GROUP BY deployment_id, tool_id
            ) a ON t.id = a.tool_id AND t.deployment_id = a.deployment_id
            WHERE t.id = $1 AND t.deployment_id = $2
            "#,
            self.tool_id,
            self.deployment_id
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(AppError::Database)?;

        let tool_type = AiToolType::from(tool.tool_type);
        let configuration: AiToolConfiguration =
            serde_json::from_value(tool.configuration).unwrap_or_default();

        Ok(AiToolWithDetails {
            id: tool.id,
            created_at: tool.created_at,
            updated_at: tool.updated_at,
            name: tool.name,
            description: tool.description,
            tool_type,
            deployment_id: tool.deployment_id,
            configuration,
        })
    }
}

impl Query for GetAiToolByIdQuery {
    type Output = AiToolWithDetails;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}

pub struct GetAgentToolsQuery {
    pub deployment_id: i64,
    pub agent_id: i64,
}

impl GetAgentToolsQuery {
    pub fn new(deployment_id: i64, agent_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
        }
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<Vec<AiTool>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let tools = sqlx::query!(
            r#"
            SELECT
                t.id, t.created_at, t.updated_at, t.name, t.description,
                t.tool_type, t.deployment_id, t.configuration
            FROM ai_tools t
            JOIN ai_agent_tools aat ON aat.tool_id = t.id
            WHERE t.deployment_id = $1 AND aat.agent_id = $2 AND aat.deployment_id = $1
            ORDER BY t.created_at DESC
            "#,
            self.deployment_id,
            self.agent_id
        )
        .fetch_all(&mut *conn)
        .await
        .map_err(AppError::Database)?;

        Ok(tools
            .into_iter()
            .map(|tool| {
                let tool_type = AiToolType::from(tool.tool_type);
                let configuration: AiToolConfiguration =
                    serde_json::from_value(tool.configuration).unwrap_or_default();

                AiTool {
                    id: tool.id,
                    created_at: tool.created_at,
                    updated_at: tool.updated_at,
                    name: tool.name,
                    description: tool.description,
                    tool_type,
                    deployment_id: tool.deployment_id,
                    configuration,
                }
            })
            .collect())
    }
}

impl Query for GetAgentToolsQuery {
    type Output = Vec<AiTool>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}

pub struct GetToolByIdQuery {
    pub tool_id: i64,
}

impl GetToolByIdQuery {
    pub fn new(tool_id: i64) -> Self {
        Self { tool_id }
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<AiTool, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let tool = sqlx::query!(
            r#"
            SELECT 
                id,
                created_at,
                updated_at,
                name,
                description,
                deployment_id,
                tool_type,
                configuration
            FROM ai_tools
            WHERE id = $1
            "#,
            self.tool_id
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => {
                AppError::NotFound(format!("Tool with id {} not found", self.tool_id))
            }
            _ => AppError::Database(e),
        })?;

        let tool_type = AiToolType::from(tool.tool_type);
        let configuration: AiToolConfiguration =
            serde_json::from_value(tool.configuration).unwrap_or_default();

        Ok(AiTool {
            id: tool.id,
            created_at: tool.created_at,
            updated_at: tool.updated_at,
            name: tool.name,
            description: tool.description,
            tool_type,
            deployment_id: tool.deployment_id,
            configuration,
        })
    }
}

impl Query for GetToolByIdQuery {
    type Output = AiTool;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}

pub struct GetAiToolsByIdsQuery {
    pub deployment_id: i64,
    pub tool_ids: Vec<i64>,
}

impl GetAiToolsByIdsQuery {
    pub fn new(deployment_id: i64, tool_ids: Vec<i64>) -> Self {
        Self {
            deployment_id,
            tool_ids,
        }
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<Vec<AiTool>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        if self.tool_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = (1..=self.tool_ids.len())
            .map(|i| format!("${}", i + 1))
            .collect::<Vec<_>>()
            .join(",");

        let query = format!(
            "SELECT id, created_at, updated_at, name, description, tool_type, deployment_id, configuration
             FROM ai_tools
             WHERE deployment_id = $1 AND id IN ({})",
            placeholders
        );

        let mut query_builder = sqlx::query(&query);
        query_builder = query_builder.bind(self.deployment_id);

        for tool_id in &self.tool_ids {
            query_builder = query_builder.bind(tool_id);
        }

        let mut conn = acquirer.acquire().await?;
        let tools = query_builder
            .fetch_all(&mut *conn)
            .await
            .map_err(|e| AppError::Database(e))?;

        Ok(tools
            .into_iter()
            .map(|row| {
                let tool_type = AiToolType::from(row.get::<String, _>("tool_type"));
                let configuration: AiToolConfiguration =
                    serde_json::from_value(row.get("configuration")).unwrap_or_default();

                AiTool {
                    id: row.get("id"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    name: row.get("name"),
                    description: row.get("description"),
                    tool_type,
                    deployment_id: row.get("deployment_id"),
                    configuration,
                }
            })
            .collect())
    }
}

impl Query for GetAiToolsByIdsQuery {
    type Output = Vec<AiTool>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}
