use sqlx::Row;

use crate::{error::AppError, models::{AiAgentWithDetails, AiAgent}, queries::Query, state::AppState};

pub struct GetAiAgentsQuery {
    pub deployment_id: i64,
    pub offset: u32,
    pub limit: u32,
    pub search: Option<String>,
}

impl GetAiAgentsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            offset: 0,
            limit: 50,
            search: None,
        }
    }

    pub fn with_limit(mut self, limit: Option<u32>) -> Self {
        if let Some(limit) = limit {
            self.limit = limit;
        }
        self
    }

    pub fn with_offset(mut self, offset: Option<u32>) -> Self {
        if let Some(offset) = offset {
            self.offset = offset;
        }
        self
    }

    pub fn with_search(mut self, search: Option<String>) -> Self {
        self.search = search;
        self
    }
}

impl Query for GetAiAgentsQuery {
    type Output = Vec<AiAgentWithDetails>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let base_query = r#"
            SELECT
                a.id, a.created_at, a.updated_at, a.name, a.description,
                a.configuration, a.deployment_id,
                COALESCE(jsonb_array_length(a.configuration->'tool_ids'), 0) as tools_count,
                COALESCE(jsonb_array_length(a.configuration->'workflow_ids'), 0) as workflows_count,
                COALESCE(jsonb_array_length(a.configuration->'knowledge_base_ids'), 0) as knowledge_bases_count
            FROM ai_agents a
            WHERE a.deployment_id = $1"#;

        let agents = if let Some(search) = &self.search {
            let query_with_search = format!("{} AND (a.name ILIKE $2 OR a.description ILIKE $2) ORDER BY a.created_at DESC LIMIT $3 OFFSET $4", base_query);
            sqlx::query(&query_with_search)
                .bind(self.deployment_id)
                .bind(format!("%{}%", search))
                .bind(self.limit as i64)
                .bind(self.offset as i64)
                .fetch_all(&app_state.db_pool)
                .await
        } else {
            let query_without_search = format!("{} ORDER BY a.created_at DESC LIMIT $2 OFFSET $3", base_query);
            sqlx::query(&query_without_search)
                .bind(self.deployment_id)
                .bind(self.limit as i64)
                .bind(self.offset as i64)
                .fetch_all(&app_state.db_pool)
                .await
        }
        .map_err(|e| AppError::Database(e))?;

        Ok(agents
            .into_iter()
            .map(|row| AiAgentWithDetails {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                name: row.get("name"),
                description: row.get("description"),
                configuration: row.get("configuration"),
                deployment_id: row.get("deployment_id"),
                tools_count: row.get::<Option<i32>, _>("tools_count").unwrap_or(0) as i64,
                workflows_count: row.get::<Option<i32>, _>("workflows_count").unwrap_or(0) as i64,
                knowledge_bases_count: row
                    .get::<Option<i32>, _>("knowledge_bases_count")
                    .unwrap_or(0) as i64,
            })
            .collect())
    }
}

pub struct GetAiAgentByIdQuery {
    pub deployment_id: i64,
    pub agent_id: i64,
}

impl GetAiAgentByIdQuery {
    pub fn new(deployment_id: i64, agent_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
        }
    }
}

impl Query for GetAiAgentByIdQuery {
    type Output = AiAgentWithDetails;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let agent = sqlx::query!(
            r#"
            SELECT
                a.id, a.created_at, a.updated_at, a.name, a.description,
                a.configuration, a.deployment_id,
                COALESCE(jsonb_array_length(a.configuration->'tool_ids'), 0) as tools_count,
                COALESCE(jsonb_array_length(a.configuration->'workflow_ids'), 0) as workflows_count,
                COALESCE(jsonb_array_length(a.configuration->'knowledge_base_ids'), 0) as knowledge_bases_count
            FROM ai_agents a
            WHERE a.id = $1 AND a.deployment_id = $2
            "#,
            self.agent_id,
            self.deployment_id
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(AiAgentWithDetails {
            id: agent.id,
            created_at: agent.created_at,
            updated_at: agent.updated_at,
            name: agent.name,
            description: agent.description,
            configuration: agent.configuration,
            deployment_id: agent.deployment_id,
            tools_count: agent.tools_count.unwrap_or(0) as i64,
            workflows_count: agent.workflows_count.unwrap_or(0) as i64,
            knowledge_bases_count: agent.knowledge_bases_count.unwrap_or(0) as i64,
        })
    }
}

pub struct GetAiAgentByNameQuery {
    pub deployment_id: i64,
    pub agent_name: String,
}

impl GetAiAgentByNameQuery {
    pub fn new(deployment_id: i64, agent_name: String) -> Self {
        Self {
            deployment_id,
            agent_name,
        }
    }
}

impl Query for GetAiAgentByNameQuery {
    type Output = AiAgent;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let agent = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, name, description, configuration, deployment_id
            FROM ai_agents
            WHERE name = $1 AND deployment_id = $2
            "#,
            self.agent_name,
            self.deployment_id
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
            configuration: agent.configuration,
            deployment_id: agent.deployment_id,
        })
    }
}
