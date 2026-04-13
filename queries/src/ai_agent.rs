use common::error::AppError;
use models::{AiAgentWithDetails, AiAgentWithFeatures};

fn parse_sub_agents(
    value: serde_json::Value,
) -> Result<Option<Vec<i64>>, AppError> {
    let parsed = serde_json::from_value::<Vec<i64>>(value)
        .map_err(|e| AppError::Internal(format!("Failed to parse sub_agents: {}", e)))?;
    Ok(Some(parsed))
}

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

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<AiAgentWithDetails>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if let Some(search) = &self.search {
            let search_pattern = format!("%{}%", search);
            let agents = sqlx::query!(
                r#"
                SELECT
                    a.id, a.created_at, a.updated_at, a.name, a.description,
                    a.configuration, a.deployment_id,
                    COALESCE((
                        SELECT jsonb_agg(rel.sub_agent_id ORDER BY rel.sub_agent_id)
                        FROM ai_agent_sub_agents rel
                        WHERE rel.deployment_id = a.deployment_id
                            AND rel.agent_id = a.id
                    ), '[]'::jsonb) as "sub_agents!: serde_json::Value",
                    COALESCE((SELECT COUNT(*) FROM ai_agent_tools aat WHERE aat.agent_id = a.id AND aat.deployment_id = a.deployment_id), 0)::bigint as "tools_count!",
                    COALESCE((SELECT COUNT(*) FROM ai_agent_knowledge_bases aakb WHERE aakb.agent_id = a.id AND aakb.deployment_id = a.deployment_id), 0)::bigint as "knowledge_bases_count!"
                FROM ai_agents a
                WHERE a.deployment_id = $1
                    AND (a.name ILIKE $2 OR a.description ILIKE $2)
                ORDER BY a.created_at DESC
                LIMIT $3 OFFSET $4
                "#,
                self.deployment_id,
                search_pattern,
                self.limit as i64,
                self.offset as i64
            )
            .fetch_all(executor)
            .await
            .map_err(AppError::Database)?;

            Ok(agents
                .into_iter()
                .map(|agent| -> Result<AiAgentWithDetails, AppError> {
                    let sub_agents = parse_sub_agents(agent.sub_agents)?;

                    Ok(AiAgentWithDetails {
                        id: agent.id,
                        created_at: agent.created_at,
                        updated_at: agent.updated_at,
                        name: agent.name,
                        description: agent.description,
                        configuration: agent.configuration,
                        deployment_id: agent.deployment_id,
                        tools_count: agent.tools_count,
                        knowledge_bases_count: agent.knowledge_bases_count,
                        sub_agents,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?)
        } else {
            let agents = sqlx::query!(
                r#"
                SELECT
                    a.id, a.created_at, a.updated_at, a.name, a.description,
                    a.configuration, a.deployment_id,
                    COALESCE((
                        SELECT jsonb_agg(rel.sub_agent_id ORDER BY rel.sub_agent_id)
                        FROM ai_agent_sub_agents rel
                        WHERE rel.deployment_id = a.deployment_id
                            AND rel.agent_id = a.id
                    ), '[]'::jsonb) as "sub_agents!: serde_json::Value",
                    COALESCE((SELECT COUNT(*) FROM ai_agent_tools aat WHERE aat.agent_id = a.id AND aat.deployment_id = a.deployment_id), 0)::bigint as "tools_count!",
                    COALESCE((SELECT COUNT(*) FROM ai_agent_knowledge_bases aakb WHERE aakb.agent_id = a.id AND aakb.deployment_id = a.deployment_id), 0)::bigint as "knowledge_bases_count!"
                FROM ai_agents a
                WHERE a.deployment_id = $1
                ORDER BY a.created_at DESC
                LIMIT $2 OFFSET $3
                "#,
                self.deployment_id,
                self.limit as i64,
                self.offset as i64
            )
            .fetch_all(executor)
            .await
            .map_err(AppError::Database)?;

            Ok(agents
                .into_iter()
                .map(|agent| -> Result<AiAgentWithDetails, AppError> {
                    let sub_agents = parse_sub_agents(agent.sub_agents)?;

                    Ok(AiAgentWithDetails {
                        id: agent.id,
                        created_at: agent.created_at,
                        updated_at: agent.updated_at,
                        name: agent.name,
                        description: agent.description,
                        configuration: agent.configuration,
                        deployment_id: agent.deployment_id,
                        tools_count: agent.tools_count,
                        knowledge_bases_count: agent.knowledge_bases_count,
                        sub_agents,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?)
        }
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<AiAgentWithDetails, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let agent = sqlx::query!(
            r#"
            SELECT
                a.id, a.created_at, a.updated_at, a.name, a.description,
                a.configuration, a.deployment_id,
                COALESCE((
                    SELECT jsonb_agg(rel.sub_agent_id ORDER BY rel.sub_agent_id)
                    FROM ai_agent_sub_agents rel
                    WHERE rel.deployment_id = a.deployment_id
                        AND rel.agent_id = a.id
                ), '[]'::jsonb) as "sub_agents!: serde_json::Value",
                COALESCE((SELECT COUNT(*) FROM ai_agent_tools aat WHERE aat.agent_id = a.id AND aat.deployment_id = a.deployment_id), 0)::bigint as "tools_count!",
                COALESCE((SELECT COUNT(*) FROM ai_agent_knowledge_bases aakb WHERE aakb.agent_id = a.id AND aakb.deployment_id = a.deployment_id), 0)::bigint as "knowledge_bases_count!"
            FROM ai_agents a
            WHERE a.id = $1 AND a.deployment_id = $2
            "#,
            self.agent_id,
            self.deployment_id
        )
        .fetch_optional(executor)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("Agent not found".to_string()))?;

        let sub_agents = parse_sub_agents(agent.sub_agents)?;

        Ok(AiAgentWithDetails {
            id: agent.id,
            created_at: agent.created_at,
            updated_at: agent.updated_at,
            name: agent.name,
            description: agent.description,
            configuration: agent.configuration,
            deployment_id: agent.deployment_id,
            tools_count: agent.tools_count,
            knowledge_bases_count: agent.knowledge_bases_count,
            sub_agents,
        })
    }
}

pub struct GetAiAgentsByIdsQuery {
    pub deployment_id: i64,
    pub agent_ids: Vec<i64>,
}

impl GetAiAgentsByIdsQuery {
    pub fn new(deployment_id: i64, agent_ids: Vec<i64>) -> Self {
        Self {
            deployment_id,
            agent_ids,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<AiAgentWithDetails>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if self.agent_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query!(
            r#"
            SELECT
                a.id, a.created_at, a.updated_at, a.name, a.description,
                a.configuration, a.deployment_id,
                COALESCE((
                    SELECT jsonb_agg(rel.sub_agent_id ORDER BY rel.sub_agent_id)
                    FROM ai_agent_sub_agents rel
                    WHERE rel.deployment_id = a.deployment_id
                        AND rel.agent_id = a.id
                ), '[]'::jsonb) as "sub_agents!: serde_json::Value",
                COALESCE((SELECT COUNT(*) FROM ai_agent_tools aat WHERE aat.agent_id = a.id AND aat.deployment_id = a.deployment_id), 0)::bigint as "tools_count!",
                COALESCE((SELECT COUNT(*) FROM ai_agent_knowledge_bases aakb WHERE aakb.agent_id = a.id AND aakb.deployment_id = a.deployment_id), 0)::bigint as "knowledge_bases_count!"
            FROM ai_agents a
            WHERE a.deployment_id = $1
              AND a.id = ANY($2::bigint[])
            ORDER BY a.name ASC
            "#,
            self.deployment_id,
            &self.agent_ids
        )
        .fetch_all(executor)
        .await
        .map_err(AppError::Database)?;

        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            let sub_agents = parse_sub_agents(row.sub_agents)?;

            result.push(AiAgentWithDetails {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                name: row.name,
                description: row.description,
                deployment_id: row.deployment_id,
                configuration: row.configuration,
                tools_count: row.tools_count,
                knowledge_bases_count: row.knowledge_bases_count,
                sub_agents,
            });
        }

        Ok(result)
    }
}

pub struct GetAiAgentByIdWithFeatures {
    pub agent_id: i64,
}

impl GetAiAgentByIdWithFeatures {
    pub fn new(agent_id: i64) -> Self {
        Self { agent_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<AiAgentWithFeatures, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT
                a.id,
                a.created_at,
                a.updated_at,
                a.description,
                a.name,
                a.deployment_id,
                a.configuration,
                COALESCE((
                    SELECT jsonb_agg(rel.sub_agent_id ORDER BY rel.sub_agent_id)
                    FROM ai_agent_sub_agents rel
                    WHERE rel.deployment_id = a.deployment_id
                        AND rel.agent_id = a.id
                ), '[]'::jsonb) as "sub_agents!: serde_json::Value",
                tools.list as "tools!: serde_json::Value",
                knowledge_bases.list as "knowledge_bases!: serde_json::Value"
            FROM ai_agents a
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
                        'requires_user_approval', t.requires_user_approval,
                        'configuration', t.configuration
                    )
                ), '[]'::jsonb) as list
                FROM ai_tools t
                JOIN ai_agent_tools at ON at.tool_id = t.id
                WHERE t.deployment_id = a.deployment_id
                    AND at.agent_id = a.id
                    AND at.deployment_id = a.deployment_id
            ) tools ON true
            LEFT JOIN LATERAL (
                SELECT COALESCE(jsonb_agg(
                    jsonb_build_object(
                        'id', kb.id::text,
                        'created_at', kb.created_at,
                        'updated_at', kb.updated_at,
                        'name', kb.name,
                        'description', kb.description,
                        'deployment_id', kb.deployment_id::text
                        ,'configuration', kb.configuration
                    )
                ), '[]'::jsonb) as list
                FROM ai_knowledge_bases kb
                JOIN ai_agent_knowledge_bases ak ON ak.knowledge_base_id = kb.id
                WHERE kb.deployment_id = a.deployment_id
                    AND ak.agent_id = a.id
                    AND ak.deployment_id = a.deployment_id
            ) knowledge_bases ON true
            WHERE a.id = $1
            "#,
            self.agent_id
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        let tools = serde_json::from_value(row.tools)
            .map_err(|e| AppError::Internal(format!("Failed to deserialize tools: {}", e)))?;
        let knowledge_bases = serde_json::from_value(row.knowledge_bases).map_err(|e| {
            AppError::Internal(format!("Failed to deserialize knowledge bases: {}", e))
        })?;
        let sub_agents = parse_sub_agents(row.sub_agents)?;

        Ok(AiAgentWithFeatures {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            description: row.description,
            name: row.name,
            deployment_id: row.deployment_id,
            configuration: row.configuration,
            tools,
            knowledge_bases,
            sub_agents,
        })
    }
}
