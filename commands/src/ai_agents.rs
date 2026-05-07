use chrono::Utc;
use common::{HasDbRouter, error::AppError};
use models::{AgentHooksConfig, AgentModelOverride, AiAgent};
use sqlx::types::Json;
use std::collections::BTreeSet;

const AGENT_NOT_FOUND: &str = "Agent not found";
const SUB_AGENT_NOT_FOUND: &str = "Sub-agent not found";
const ERR_INVALID_TOOL_IDS: &str = "One or more tool IDs are invalid for this deployment";
const ERR_INVALID_KB_IDS: &str = "One or more knowledge base IDs are invalid for this deployment";

pub struct CreateAiAgentCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub configuration: serde_json::Value,
    pub tool_ids: Option<Vec<i64>>,
    pub knowledge_base_ids: Option<Vec<i64>>,
    pub sub_agents: Option<Vec<i64>>,
    pub strong_model: Option<AgentModelOverride>,
    pub weak_model: Option<AgentModelOverride>,
    pub hooks: Option<AgentHooksConfig>,
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
            strong_model: None,
            weak_model: None,
            hooks: None,
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

    pub fn with_strong_model(mut self, override_: AgentModelOverride) -> Self {
        self.strong_model = Some(override_);
        self
    }

    pub fn with_weak_model(mut self, override_: AgentModelOverride) -> Self {
        self.weak_model = Some(override_);
        self
    }

    pub fn with_hooks(mut self, hooks: AgentHooksConfig) -> Self {
        self.hooks = Some(hooks);
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
        let sub_agent_ids = self.sub_agents.unwrap_or_default();
        let sanitized_configuration = sanitize_configuration(self.configuration);

        validate_model_override("strong_model", self.strong_model.as_ref())?;
        validate_model_override("weak_model", self.weak_model.as_ref())?;
        if let Some(hooks) = &self.hooks {
            validate_hooks(hooks)?;
        }

        let strong_provider = self
            .strong_model
            .as_ref()
            .map(|o| o.provider.trim().to_string());
        let strong_model = self
            .strong_model
            .as_ref()
            .map(|o| o.model.trim().to_string());
        let weak_provider = self
            .weak_model
            .as_ref()
            .map(|o| o.provider.trim().to_string());
        let weak_model = self.weak_model.as_ref().map(|o| o.model.trim().to_string());
        let hooks_value = Json(self.hooks.clone().unwrap_or_default());

        let mut tx = deps
            .db_router()
            .writer()
            .begin()
            .await
            .map_err(AppError::Database)?;

        let agent = sqlx::query!(
            r#"
            INSERT INTO ai_agents (
                id, created_at, updated_at, name, description, deployment_id,
                configuration, strong_model_provider, strong_model,
                weak_model_provider, weak_model, hooks
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING id, created_at, updated_at, name, description, deployment_id,
                      configuration,
                      strong_model_provider, strong_model,
                      weak_model_provider, weak_model,
                      hooks as "hooks!: Json<AgentHooksConfig>"
            "#,
            agent_id,
            now,
            now,
            self.name,
            self.description,
            self.deployment_id,
            sanitized_configuration,
            strong_provider,
            strong_model,
            weak_provider,
            weak_model,
            hooks_value as _,
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
            &sub_agent_ids,
        )
        .await?;

        tx.commit().await.map_err(AppError::Database)?;

        Ok(AiAgent {
            id: agent.id,
            created_at: agent.created_at,
            updated_at: agent.updated_at,
            name: agent.name,
            description: agent.description,
            deployment_id: agent.deployment_id,
            configuration: agent.configuration,
            sub_agents: Some(sub_agent_ids),
            strong_model: build_override(agent.strong_model_provider, agent.strong_model),
            weak_model: build_override(agent.weak_model_provider, agent.weak_model),
            hooks: agent.hooks.0,
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
    pub strong_model: Option<AgentModelOverride>,
    pub clear_strong_model: bool,
    pub weak_model: Option<AgentModelOverride>,
    pub clear_weak_model: bool,
    pub hooks: Option<AgentHooksConfig>,
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
            strong_model: None,
            clear_strong_model: false,
            weak_model: None,
            clear_weak_model: false,
            hooks: None,
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

    pub fn with_strong_model(mut self, override_: AgentModelOverride) -> Self {
        self.strong_model = Some(override_);
        self.clear_strong_model = false;
        self
    }

    pub fn clearing_strong_model(mut self) -> Self {
        self.strong_model = None;
        self.clear_strong_model = true;
        self
    }

    pub fn with_weak_model(mut self, override_: AgentModelOverride) -> Self {
        self.weak_model = Some(override_);
        self.clear_weak_model = false;
        self
    }

    pub fn clearing_weak_model(mut self) -> Self {
        self.weak_model = None;
        self.clear_weak_model = true;
        self
    }

    pub fn with_hooks(mut self, hooks: AgentHooksConfig) -> Self {
        self.hooks = Some(hooks);
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

        validate_model_override("strong_model", self.strong_model.as_ref())?;
        validate_model_override("weak_model", self.weak_model.as_ref())?;
        if self.clear_strong_model && self.strong_model.is_some() {
            return Err(AppError::BadRequest(
                "clear_strong_model cannot be combined with a new strong_model".to_string(),
            ));
        }
        if self.clear_weak_model && self.weak_model.is_some() {
            return Err(AppError::BadRequest(
                "clear_weak_model cannot be combined with a new weak_model".to_string(),
            ));
        }
        if let Some(hooks) = &self.hooks {
            validate_hooks(hooks)?;
        }

        let strong_provider = self
            .strong_model
            .as_ref()
            .map(|o| o.provider.trim().to_string());
        let strong_model = self
            .strong_model
            .as_ref()
            .map(|o| o.model.trim().to_string());
        let weak_provider = self
            .weak_model
            .as_ref()
            .map(|o| o.provider.trim().to_string());
        let weak_model = self.weak_model.as_ref().map(|o| o.model.trim().to_string());
        let hooks_value = self.hooks.clone().map(Json);

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
                strong_model_provider = CASE
                    WHEN $7::bool THEN NULL
                    WHEN $8::text IS NOT NULL THEN $8
                    ELSE strong_model_provider
                END,
                strong_model = CASE
                    WHEN $7::bool THEN NULL
                    WHEN $9::text IS NOT NULL THEN $9
                    ELSE strong_model
                END,
                weak_model_provider = CASE
                    WHEN $10::bool THEN NULL
                    WHEN $11::text IS NOT NULL THEN $11
                    ELSE weak_model_provider
                END,
                weak_model = CASE
                    WHEN $10::bool THEN NULL
                    WHEN $12::text IS NOT NULL THEN $12
                    ELSE weak_model
                END,
                hooks = COALESCE($13, hooks)
            WHERE id = $5 AND deployment_id = $6
            RETURNING id, created_at, updated_at, name, description, deployment_id,
                      configuration,
                      strong_model_provider, strong_model,
                      weak_model_provider, weak_model,
                      hooks as "hooks!: Json<AgentHooksConfig>"
            "#,
            now,
            self.name,
            self.description,
            configuration,
            agent_id,
            deployment_id,
            self.clear_strong_model,
            strong_provider,
            strong_model,
            self.clear_weak_model,
            weak_provider,
            weak_model,
            hooks_value as _,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        if let Some(tool_ids) = self.tool_ids {
            replace_agent_tools(&mut tx, agent_id, deployment_id, &tool_ids).await?;
        }

        if let Some(knowledge_base_ids) = self.knowledge_base_ids {
            replace_agent_knowledge_bases(&mut tx, agent_id, deployment_id, &knowledge_base_ids)
                .await?;
        }
        if let Some(sub_agent_ids) = self.sub_agents.as_ref() {
            replace_agent_sub_agents(&mut tx, agent_id, deployment_id, sub_agent_ids).await?;
        }

        tx.commit().await.map_err(AppError::Database)?;

        Ok(AiAgent {
            id: agent.id,
            created_at: agent.created_at,
            updated_at: agent.updated_at,
            name: agent.name,
            description: agent.description,
            deployment_id: agent.deployment_id,
            configuration: agent.configuration,
            sub_agents: self.sub_agents,
            strong_model: build_override(agent.strong_model_provider, agent.strong_model),
            weak_model: build_override(agent.weak_model_provider, agent.weak_model),
            hooks: agent.hooks.0,
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

        sqlx::query!(
            "INSERT INTO ai_agent_sub_agents (deployment_id, agent_id, sub_agent_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            self.deployment_id,
            self.agent_id,
            self.sub_agent_id,
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

        sqlx::query!(
            "DELETE FROM ai_agent_sub_agents WHERE deployment_id = $1 AND agent_id = $2 AND sub_agent_id = $3",
            self.deployment_id,
            self.agent_id,
            self.sub_agent_id
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

        sqlx::query!(
            "DELETE FROM ai_agent_sub_agents WHERE deployment_id = $1 AND (agent_id = $2 OR sub_agent_id = $2)",
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
    sub_agent_ids: &[i64],
) -> Result<(), AppError> {
    replace_agent_tools(tx, agent_id, deployment_id, tool_ids).await?;
    replace_agent_knowledge_bases(tx, agent_id, deployment_id, knowledge_base_ids).await?;
    replace_agent_sub_agents(tx, agent_id, deployment_id, sub_agent_ids).await?;
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

async fn replace_agent_sub_agents(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    agent_id: i64,
    deployment_id: i64,
    sub_agent_ids: &[i64],
) -> Result<(), AppError> {
    validate_sub_agent_ids(tx, agent_id, deployment_id, sub_agent_ids).await?;

    sqlx::query!(
        "DELETE FROM ai_agent_sub_agents WHERE deployment_id = $1 AND agent_id = $2",
        deployment_id,
        agent_id
    )
    .execute(&mut **tx)
    .await
    .map_err(AppError::Database)?;

    for sub_agent_id in sub_agent_ids {
        sqlx::query!(
            "INSERT INTO ai_agent_sub_agents (deployment_id, agent_id, sub_agent_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            deployment_id,
            agent_id,
            sub_agent_id
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
    let unique_ids = dedupe_ids(ids);
    if unique_ids.is_empty() {
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
        &unique_ids
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::Database)?
    .unwrap_or(0);

    if valid_count != unique_ids.len() as i64 {
        return Err(AppError::BadRequest(ERR_INVALID_TOOL_IDS.to_string()));
    }

    Ok(())
}

async fn validate_knowledge_base_ids(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    deployment_id: i64,
    ids: &[i64],
) -> Result<(), AppError> {
    let unique_ids = dedupe_ids(ids);
    if unique_ids.is_empty() {
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
        &unique_ids
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::Database)?
    .unwrap_or(0);

    if valid_count != unique_ids.len() as i64 {
        return Err(AppError::BadRequest(ERR_INVALID_KB_IDS.to_string()));
    }

    Ok(())
}

async fn validate_sub_agent_ids(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    agent_id: i64,
    deployment_id: i64,
    ids: &[i64],
) -> Result<(), AppError> {
    if ids.iter().any(|id| *id == agent_id) {
        return Err(AppError::BadRequest(
            "An agent cannot be attached as its own sub-agent".to_string(),
        ));
    }
    let unique_ids = dedupe_ids(ids);
    if unique_ids.is_empty() {
        return Ok(());
    }

    let valid_count = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::bigint
        FROM ai_agents
        WHERE deployment_id = $1
            AND id = ANY($2::bigint[])
        "#,
        deployment_id,
        &unique_ids
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::Database)?
    .unwrap_or(0);

    if valid_count != unique_ids.len() as i64 {
        return Err(AppError::BadRequest(SUB_AGENT_NOT_FOUND.to_string()));
    }

    Ok(())
}

fn dedupe_ids(ids: &[i64]) -> Vec<i64> {
    ids.iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn validate_model_override(
    field: &'static str,
    override_: Option<&AgentModelOverride>,
) -> Result<(), AppError> {
    let Some(o) = override_ else { return Ok(()) };
    if o.provider.trim().is_empty() {
        return Err(AppError::BadRequest(format!(
            "{field}.provider is required when {field} is set"
        )));
    }
    if o.model.trim().is_empty() {
        return Err(AppError::BadRequest(format!(
            "{field}.model is required when {field} is set"
        )));
    }
    Ok(())
}

fn validate_hooks(hooks: &AgentHooksConfig) -> Result<(), AppError> {
    for (kind, steps) in [
        ("execution_start", &hooks.execution_start),
        ("execution_end", &hooks.execution_end),
    ] {
        for (i, step) in steps.iter().enumerate() {
            if step.tool_name.trim().is_empty() {
                return Err(AppError::BadRequest(format!(
                    "hooks.{kind}[{i}].tool_name must not be empty"
                )));
            }
            if !step.args.is_object() && !step.args.is_null() {
                return Err(AppError::BadRequest(format!(
                    "hooks.{kind}[{i}].args must be a JSON object"
                )));
            }
        }
    }
    Ok(())
}

fn build_override(provider: Option<String>, model: Option<String>) -> Option<AgentModelOverride> {
    match (provider, model) {
        (Some(p), Some(m)) => Some(AgentModelOverride {
            provider: p,
            model: m,
        }),
        _ => None,
    }
}
