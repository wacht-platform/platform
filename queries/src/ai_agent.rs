use sqlx::Row;

use crate::Query;
use common::error::AppError;
use common::state::AppState;
use models::{AiAgent, AiAgentWithDetails, AiAgentWithFeatures};

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

pub struct GetAiAgentByNameWithFeatures {
    pub deployment_id: i64,
    pub agent_name: String,
}

impl GetAiAgentByNameWithFeatures {
    pub fn new(deployment_id: i64, agent_name: String) -> Self {
        Self {
            deployment_id,
            agent_name,
        }
    }
}

impl Query for GetAiAgentByNameWithFeatures {
    type Output = AiAgentWithFeatures;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query(
            r#"
            SELECT
                a.id,
                a.created_at,
                a.updated_at,
                a.name,
                a.description,
                a.deployment_id,
                a.configuration,
                tools.list as tools,
                workflows.list as workflows,
                knowledge_bases.list as knowledge_bases,
                integrations.list as integrations
            FROM
                ai_agents a
            LEFT JOIN LATERAL (
                SELECT COALESCE(jsonb_agg(
                    jsonb_build_object(
                        'id', t.id::text,
                        'created_at', t.created_at,
                        'updated_at', t.updated_at,
                        'name', t.name,
                        'description', t.description,
                        'tool_type', t.tool_type,
                        'deployment_id', t.deployment_id::text,
                        'configuration', t.configuration
                    )
                ), '[]'::jsonb) as list
                FROM ai_tools t
                WHERE t.deployment_id = a.deployment_id
                    AND jsonb_typeof(a.configuration->'tool_ids') = 'array'
                    AND t.id IN (SELECT value::bigint FROM jsonb_array_elements_text(a.configuration->'tool_ids'))
            ) tools ON true
            LEFT JOIN LATERAL (
                SELECT COALESCE(jsonb_agg(
                    jsonb_build_object(
                        'id', w.id::text,
                        'created_at', w.created_at,
                        'updated_at', w.updated_at,
                        'name', w.name,
                        'description', w.description,
                        'deployment_id', w.deployment_id::text,
                        'configuration', w.configuration,
                        'workflow_definition', w.workflow_definition
                    )
                ), '[]'::jsonb) as list
                FROM ai_workflows w
                WHERE w.deployment_id = a.deployment_id
                    AND jsonb_typeof(a.configuration->'workflow_ids') = 'array'
                    AND w.id IN (SELECT value::bigint FROM jsonb_array_elements_text(a.configuration->'workflow_ids'))
            ) workflows ON true
            LEFT JOIN LATERAL (
                SELECT COALESCE(jsonb_agg(
                    jsonb_build_object(
                        'id', k.id::text,
                        'created_at', k.created_at,
                        'updated_at', k.updated_at,
                        'name', k.name,
                        'description', k.description,
                        'deployment_id', k.deployment_id::text,
                        'configuration', k.configuration
                    )
                ), '[]'::jsonb) as list
                FROM ai_knowledge_bases k
                WHERE k.deployment_id = a.deployment_id
                    AND jsonb_typeof(a.configuration->'knowledge_base_ids') = 'array'
                    AND k.id IN (SELECT value::bigint FROM jsonb_array_elements_text(a.configuration->'knowledge_base_ids'))
            ) knowledge_bases ON true
            LEFT JOIN LATERAL (
                SELECT COALESCE(jsonb_agg(
                    jsonb_build_object(
                        'id', i.id::text,
                        'created_at', i.created_at,
                        'updated_at', i.updated_at,
                        'name', i.name,
                        'deployment_id', i.deployment_id::text,
                        'integration_type', i.integration_type,
                        'config', i.config,
                        'enabled', i.enabled
                    )
                ), '[]'::jsonb) as list
                FROM agent_integrations i
                WHERE i.deployment_id = a.deployment_id
                    AND jsonb_typeof(a.configuration->'integration_ids') = 'array'
                    AND i.id IN (SELECT value::bigint FROM jsonb_array_elements_text(a.configuration->'integration_ids'))
            ) integrations ON true
            WHERE
                a.name = $1 AND a.deployment_id = $2
            "#,
        )
        .bind(&self.agent_name)
        .bind(self.deployment_id)
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        let tools = serde_json::from_value(row.get("tools"))
            .map_err(|e| AppError::Internal(format!("Failed to deserialize tools: {}", e)))?;
        let workflows = serde_json::from_value(row.get("workflows"))
            .map_err(|e| AppError::Internal(format!("Failed to deserialize workflows: {}", e)))?;
        let knowledge_bases = serde_json::from_value(row.get("knowledge_bases")).map_err(|e| {
            AppError::Internal(format!("Failed to deserialize knowledge bases: {}", e))
        })?;
        let integrations = serde_json::from_value(row.get("integrations")).map_err(|e| {
            AppError::Internal(format!("Failed to deserialize integrations: {}", e))
        })?;

        Ok(AiAgentWithFeatures {
            id: row.get("id"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            description: row.get("description"),
            name: row.get("name"),
            deployment_id: row.get("deployment_id"),
            configuration: row.get("configuration"),
            tools,
            workflows,
            knowledge_bases,
            integrations,
        })
    }
}
