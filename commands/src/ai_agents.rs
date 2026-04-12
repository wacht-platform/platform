use chrono::Utc;
use common::{HasDbRouter, error::AppError};
use models::AiAgent;
use serde::de::DeserializeOwned;

const AGENT_NOT_FOUND: &str = "Agent not found";
const SUB_AGENT_NOT_FOUND: &str = "Sub-agent not found";
const ERR_SERIALIZE_SUB_AGENTS: &str = "Failed to serialize sub_agents";
const ERR_INVALID_TOOL_IDS: &str = "One or more tool IDs are invalid for this deployment";
const ERR_INVALID_KB_IDS: &str = "One or more knowledge base IDs are invalid for this deployment";

fn parse_optional_json<T: DeserializeOwned>(
    value: Option<serde_json::Value>,
    field: &str,
) -> Result<Option<T>, AppError> {
    value
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Failed to parse {}: {}", field, e)))
        })
        .transpose()
}

fn serialize_sub_agents(sub_agents: Vec<i64>) -> Result<serde_json::Value, AppError> {
    serde_json::to_value(sub_agents)
        .map_err(|e| AppError::Internal(format!("{ERR_SERIALIZE_SUB_AGENTS}: {}", e)))
}

pub struct CreateAiAgentCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub configuration: serde_json::Value,
    pub tool_ids: Option<Vec<i64>>,
    pub knowledge_base_ids: Option<Vec<i64>>,
    pub sub_agents: Option<Vec<i64>>,
    pub spawn_config: Option<models::SpawnConfig>,
}

impl CreateAiAgentCommand {
    pub fn new(
        id: i64,
        deployment_id: i64,
        name: String,
        description: Option<String>,
        configuration: serde_json::Value,
    ) -> Self {
        Self {
            id,
            deployment_id,
            name,
            description,
            configuration,
            tool_ids: None,
            knowledge_base_ids: None,
            sub_agents: None,
            spawn_config: None,
        }
    }

    pub fn with_tool_ids(mut self, tool_ids: Vec<i64>) -> Self {
        self.tool_ids = Some(tool_ids);
        self
    }

    pub fn with_knowledge_base_ids(mut self, knowledge_base_ids: Vec<i64>) -> Self {
        self.knowledge_base_ids = Some(knowledge_base_ids);
        self
    }

    pub fn with_sub_agents(mut self, sub_agents: Vec<i64>) -> Self {
        self.sub_agents = Some(sub_agents);
        self
    }

    pub fn with_spawn_config(mut self, spawn_config: models::SpawnConfig) -> Self {
        self.spawn_config = Some(spawn_config);
        self
    }
}

impl CreateAiAgentCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<AiAgent, AppError>
    where
        D: HasDbRouter,
    {
        let now = Utc::now();
        let agent_id = self.id;
        let tool_ids = self.tool_ids.unwrap_or_default();
        let knowledge_base_ids = self.knowledge_base_ids.unwrap_or_default();
        let sanitized_configuration = sanitize_configuration(self.configuration);

        let sub_agents_json = self
            .sub_agents
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| AppError::Serialization(e.to_string()))?;
        let spawn_config_json = self
            .spawn_config
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        let mut tx = deps
            .db_router()
            .writer()
            .begin()
            .await
            .map_err(AppError::Database)?;

        let agent = sqlx::query!(
            r#"
            INSERT INTO ai_agents (id, created_at, updated_at, name, description, deployment_id, configuration, sub_agents, spawn_config)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING id, created_at, updated_at, name, description, deployment_id, configuration, sub_agents, spawn_config
            "#,
            agent_id,
            now,
            now,
            self.name,
            self.description,
            self.deployment_id,
            sanitized_configuration,
            sub_agents_json,
            spawn_config_json,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        sync_agent_relations(
            &mut tx,
            agent_id,
            self.deployment_id,
            &tool_ids,
            &knowledge_base_ids,
        )
        .await?;

        tx.commit().await.map_err(AppError::Database)?;

        let sub_agents = parse_optional_json(agent.sub_agents, "sub_agents")?;
        let spawn_config = parse_optional_json(agent.spawn_config, "spawn_config")?;

        Ok(AiAgent {
            id: agent.id,
            created_at: agent.created_at,
            updated_at: agent.updated_at,
            name: agent.name,
            description: agent.description,
            deployment_id: agent.deployment_id,
            configuration: agent.configuration,
            sub_agents,
            spawn_config,
        })
    }
}

pub struct UpdateAiAgentCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub configuration: Option<serde_json::Value>,
    pub tool_ids: Option<Vec<i64>>,
    pub knowledge_base_ids: Option<Vec<i64>>,
    pub sub_agents: Option<Vec<i64>>,
    pub spawn_config: Option<models::SpawnConfig>,
}

impl UpdateAiAgentCommand {
    pub fn new(deployment_id: i64, agent_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
            name: None,
            description: None,
            configuration: None,
            tool_ids: None,
            knowledge_base_ids: None,
            sub_agents: None,
            spawn_config: None,
        }
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }

    pub fn with_configuration(mut self, configuration: serde_json::Value) -> Self {
        self.configuration = Some(configuration);
        self
    }

    pub fn with_tool_ids(mut self, tool_ids: Vec<i64>) -> Self {
        self.tool_ids = Some(tool_ids);
        self
    }

    pub fn with_knowledge_base_ids(mut self, knowledge_base_ids: Vec<i64>) -> Self {
        self.knowledge_base_ids = Some(knowledge_base_ids);
        self
    }

    pub fn with_sub_agents(mut self, sub_agents: Vec<i64>) -> Self {
        self.sub_agents = Some(sub_agents);
        self
    }

    pub fn with_spawn_config(mut self, spawn_config: models::SpawnConfig) -> Self {
        self.spawn_config = Some(spawn_config);
        self
    }
}

impl UpdateAiAgentCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<AiAgent, AppError>
    where
        D: HasDbRouter,
    {
        let now = Utc::now();
        let agent_id = self.agent_id;
        let deployment_id = self.deployment_id;
        let configuration = self.configuration.map(sanitize_configuration);
        let sub_agents_json = self
            .sub_agents
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| AppError::Serialization(e.to_string()))?;
        let spawn_config_json = self
            .spawn_config
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        let mut tx = deps
            .db_router()
            .writer()
            .begin()
            .await
            .map_err(AppError::Database)?;

        let agent = sqlx::query!(
            r#"
            UPDATE ai_agents
            SET
                updated_at = $1,
                name = COALESCE($2, name),
                description = COALESCE($3, description),
                configuration = COALESCE($4, configuration),
                sub_agents = COALESCE($5, sub_agents),
                spawn_config = COALESCE($6, spawn_config)
            WHERE id = $7 AND deployment_id = $8
            RETURNING id, created_at, updated_at, name, description, deployment_id, configuration, sub_agents, spawn_config
            "#,
            now,
            self.name,
            self.description,
            configuration,
            sub_agents_json,
            spawn_config_json,
            agent_id,
            deployment_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        if let Some(tool_ids) = self.tool_ids {
            replace_agent_tools(&mut tx, agent_id, deployment_id, &tool_ids).await?;
        }

        if let Some(knowledge_base_ids) = self.knowledge_base_ids {
            replace_agent_knowledge_bases(
                &mut tx,
                agent_id,
                deployment_id,
                &knowledge_base_ids,
            )
            .await?;
        }

        tx.commit().await.map_err(AppError::Database)?;

        let sub_agents = parse_optional_json(agent.sub_agents, "sub_agents")?;
        let spawn_config = parse_optional_json(agent.spawn_config, "spawn_config")?;

        Ok(AiAgent {
            id: agent.id,
            created_at: agent.created_at,
            updated_at: agent.updated_at,
            name: agent.name,
            description: agent.description,
            deployment_id: agent.deployment_id,
            configuration: agent.configuration,
            sub_agents,
            spawn_config,
        })
    }
}

pub struct AttachToolToAgentCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub tool_id: i64,
}

impl AttachToolToAgentCommand {
    pub fn new(deployment_id: i64, agent_id: i64, tool_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
            tool_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            INSERT INTO ai_agent_tools (deployment_id, agent_id, tool_id)
            SELECT $1, a.id, t.id
            FROM ai_agents a
            JOIN ai_tools t ON t.id = $3 AND t.deployment_id = $1
            WHERE a.id = $2 AND a.deployment_id = $1
            ON CONFLICT DO NOTHING
            "#,
            self.deployment_id,
            self.agent_id,
            self.tool_id
        )
        .execute(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(())
    }
}

pub struct DetachToolFromAgentCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub tool_id: i64,
}

impl DetachToolFromAgentCommand {
    pub fn new(deployment_id: i64, agent_id: i64, tool_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
            tool_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            DELETE FROM ai_agent_tools aat
            USING ai_agents a
            WHERE aat.agent_id = a.id
              AND aat.deployment_id = $3
              AND a.id = $1
              AND aat.tool_id = $2
              AND a.deployment_id = $3
            "#,
            self.agent_id,
            self.tool_id,
            self.deployment_id
        )
        .execute(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(())
    }
}

pub struct AttachKnowledgeBaseToAgentCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub knowledge_base_id: i64,
}

impl AttachKnowledgeBaseToAgentCommand {
    pub fn new(deployment_id: i64, agent_id: i64, knowledge_base_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
            knowledge_base_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            INSERT INTO ai_agent_knowledge_bases (deployment_id, agent_id, knowledge_base_id)
            SELECT $1, a.id, kb.id
            FROM ai_agents a
            JOIN ai_knowledge_bases kb ON kb.id = $3 AND kb.deployment_id = $1
            WHERE a.id = $2 AND a.deployment_id = $1
            ON CONFLICT DO NOTHING
            "#,
            self.deployment_id,
            self.agent_id,
            self.knowledge_base_id
        )
        .execute(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(())
    }
}

pub struct DetachKnowledgeBaseFromAgentCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub knowledge_base_id: i64,
}

impl DetachKnowledgeBaseFromAgentCommand {
    pub fn new(deployment_id: i64, agent_id: i64, knowledge_base_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
            knowledge_base_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            DELETE FROM ai_agent_knowledge_bases aakb
            USING ai_agents a
            WHERE aakb.agent_id = a.id
              AND aakb.deployment_id = $3
              AND a.id = $1
              AND aakb.knowledge_base_id = $2
              AND a.deployment_id = $3
            "#,
            self.agent_id,
            self.knowledge_base_id,
            self.deployment_id
        )
        .execute(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(())
    }
}

pub struct AttachSubAgentToAgentCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub sub_agent_id: i64,
}

impl AttachSubAgentToAgentCommand {
    pub fn new(deployment_id: i64, agent_id: i64, sub_agent_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
            sub_agent_id,
        }
    }
}

impl AttachSubAgentToAgentCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter,
    {
        if self.agent_id == self.sub_agent_id {
            return Err(AppError::BadRequest(
                "An agent cannot be attached as its own sub-agent".to_string(),
            ));
        }

        let mut tx = deps
            .db_router()
            .writer()
            .begin()
            .await
            .map_err(AppError::Database)?;

        let parent_exists: Option<i64> = sqlx::query_scalar!(
            "SELECT id FROM ai_agents WHERE id = $1 AND deployment_id = $2",
            self.agent_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        if parent_exists.is_none() {
            return Err(AppError::NotFound(AGENT_NOT_FOUND.to_string()));
        }

        let child_exists: Option<i64> = sqlx::query_scalar!(
            "SELECT id FROM ai_agents WHERE id = $1 AND deployment_id = $2",
            self.sub_agent_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        if child_exists.is_none() {
            return Err(AppError::NotFound(SUB_AGENT_NOT_FOUND.to_string()));
        }

        let sub_agents_json: Option<serde_json::Value> = sqlx::query_scalar!(
            r#"
            SELECT sub_agents as "sub_agents: serde_json::Value"
            FROM ai_agents
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.agent_id,
            self.deployment_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        let mut sub_agents: Vec<i64> =
            parse_optional_json(sub_agents_json, "sub_agents")?.unwrap_or_default();

        if !sub_agents.contains(&self.sub_agent_id) {
            sub_agents.push(self.sub_agent_id);
        }

        let updated_sub_agents = serialize_sub_agents(sub_agents)?;

        sqlx::query!(
            "UPDATE ai_agents SET sub_agents = $1, updated_at = NOW() WHERE id = $2 AND deployment_id = $3",
            updated_sub_agents,
            self.agent_id,
            self.deployment_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        tx.commit().await.map_err(AppError::Database)?;

        Ok(())
    }
}

pub struct DetachSubAgentFromAgentCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub sub_agent_id: i64,
}

impl DetachSubAgentFromAgentCommand {
    pub fn new(deployment_id: i64, agent_id: i64, sub_agent_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
            sub_agent_id,
        }
    }
}

impl DetachSubAgentFromAgentCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter,
    {
        let mut tx = deps
            .db_router()
            .writer()
            .begin()
            .await
            .map_err(AppError::Database)?;

        let sub_agents_json: Option<serde_json::Value> = sqlx::query_scalar!(
            r#"
            SELECT sub_agents as "sub_agents: serde_json::Value"
            FROM ai_agents
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.agent_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound(AGENT_NOT_FOUND.to_string()))?;

        let mut sub_agents: Vec<i64> =
            parse_optional_json(sub_agents_json, "sub_agents")?.unwrap_or_default();

        sub_agents.retain(|id| *id != self.sub_agent_id);

        let updated_sub_agents = serialize_sub_agents(sub_agents)?;

        sqlx::query!(
            "UPDATE ai_agents SET sub_agents = $1, updated_at = NOW() WHERE id = $2 AND deployment_id = $3",
            updated_sub_agents,
            self.agent_id,
            self.deployment_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        tx.commit().await.map_err(AppError::Database)?;

        Ok(())
    }
}

pub struct DeleteAiAgentCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
}

impl DeleteAiAgentCommand {
    pub fn new(deployment_id: i64, agent_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
        }
    }
}

impl DeleteAiAgentCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter,
    {
        let mut tx = deps
            .db_router()
            .writer()
            .begin()
            .await
            .map_err(AppError::Database)?;

        // Delete all agent relationships first
        sqlx::query!(
            "DELETE FROM ai_agent_tools WHERE deployment_id = $1 AND agent_id = $2",
            self.deployment_id,
            self.agent_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        sqlx::query!(
            "DELETE FROM ai_agent_knowledge_bases WHERE deployment_id = $1 AND agent_id = $2",
            self.deployment_id,
            self.agent_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        // Delete the agent
        sqlx::query!(
            "DELETE FROM ai_agents WHERE id = $1 AND deployment_id = $2",
            self.agent_id,
            self.deployment_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        tx.commit().await.map_err(AppError::Database)?;

        Ok(())
    }
}

async fn sync_agent_relations(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    agent_id: i64,
    deployment_id: i64,
    tool_ids: &[i64],
    knowledge_base_ids: &[i64],
) -> Result<(), AppError> {
    replace_agent_tools(tx, agent_id, deployment_id, tool_ids).await?;
    replace_agent_knowledge_bases(tx, agent_id, deployment_id, knowledge_base_ids).await?;
    Ok(())
}

async fn replace_agent_tools(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    agent_id: i64,
    deployment_id: i64,
    tool_ids: &[i64],
) -> Result<(), AppError> {
    validate_tool_ids(tx, deployment_id, tool_ids).await?;

    sqlx::query!(
        "DELETE FROM ai_agent_tools WHERE deployment_id = $1 AND agent_id = $2",
        deployment_id,
        agent_id
    )
    .execute(&mut **tx)
    .await
    .map_err(AppError::Database)?;

    for tool_id in tool_ids {
        sqlx::query!(
            "INSERT INTO ai_agent_tools (deployment_id, agent_id, tool_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            deployment_id,
            agent_id,
            tool_id
        )
        .execute(&mut **tx)
        .await
        .map_err(AppError::Database)?;
    }

    Ok(())
}

async fn replace_agent_knowledge_bases(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    agent_id: i64,
    deployment_id: i64,
    knowledge_base_ids: &[i64],
) -> Result<(), AppError> {
    validate_knowledge_base_ids(tx, deployment_id, knowledge_base_ids).await?;

    sqlx::query!(
        "DELETE FROM ai_agent_knowledge_bases WHERE deployment_id = $1 AND agent_id = $2",
        deployment_id,
        agent_id
    )
    .execute(&mut **tx)
    .await
    .map_err(AppError::Database)?;

    for knowledge_base_id in knowledge_base_ids {
        sqlx::query!(
            "INSERT INTO ai_agent_knowledge_bases (deployment_id, agent_id, knowledge_base_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            deployment_id,
            agent_id,
            knowledge_base_id
        )
        .execute(&mut **tx)
        .await
        .map_err(AppError::Database)?;
    }

    Ok(())
}

fn sanitize_configuration(mut configuration: serde_json::Value) -> serde_json::Value {
    if let Some(object) = configuration.as_object_mut() {
        object.remove("tool_ids");
        object.remove("knowledge_base_ids");
    }
    configuration
}

async fn validate_tool_ids(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    deployment_id: i64,
    ids: &[i64],
) -> Result<(), AppError> {
    if ids.is_empty() {
        return Ok(());
    }

    let valid_count = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::bigint
        FROM ai_tools
        WHERE deployment_id = $1
            AND id = ANY($2::bigint[])
        "#,
        deployment_id,
        ids
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::Database)?
    .unwrap_or(0);

    if valid_count != ids.len() as i64 {
        return Err(AppError::BadRequest(ERR_INVALID_TOOL_IDS.to_string()));
    }

    Ok(())
}

async fn validate_knowledge_base_ids(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    deployment_id: i64,
    ids: &[i64],
) -> Result<(), AppError> {
    if ids.is_empty() {
        return Ok(());
    }

    let valid_count = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::bigint
        FROM ai_knowledge_bases
        WHERE deployment_id = $1
            AND id = ANY($2::bigint[])
        "#,
        deployment_id,
        ids
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::Database)?
    .unwrap_or(0);

    if valid_count != ids.len() as i64 {
        return Err(AppError::BadRequest(ERR_INVALID_KB_IDS.to_string()));
    }

    Ok(())
}
