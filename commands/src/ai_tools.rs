use crate::dynamic_update_set::DynamicUpdateSet;
use common::error::AppError;
use models::{AiTool, AiToolConfiguration, AiToolType, CodeRunnerEnvVariable};

use chrono::Utc;
use sqlx::Row;

pub struct CreateAiToolCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub tool_type: AiToolType,
    pub requires_user_approval: bool,
    pub configuration: AiToolConfiguration,
}

impl CreateAiToolCommand {
    pub fn new(
        id: i64,
        deployment_id: i64,
        name: String,
        description: Option<String>,
        tool_type: AiToolType,
        requires_user_approval: bool,
        configuration: AiToolConfiguration,
    ) -> Self {
        Self {
            id,
            deployment_id,
            name,
            description,
            tool_type,
            requires_user_approval,
            configuration,
        }
    }

    async fn validate(&self) -> Result<(), AppError> {
        if self.name.trim().is_empty() {
            return Err(AppError::BadRequest("Tool name is required".to_string()));
        }

        match &self.configuration {
            AiToolConfiguration::Api(config) => {
                if config.endpoint.trim().is_empty() {
                    return Err(AppError::BadRequest("API endpoint is required".to_string()));
                }
                if !config.endpoint.starts_with("http://")
                    && !config.endpoint.starts_with("https://")
                {
                    return Err(AppError::BadRequest(
                        "API endpoint must be a valid URL (http:// or https://)".to_string(),
                    ));
                }
            }
            AiToolConfiguration::PlatformEvent(config) => {
                if config.event_label.trim().is_empty() {
                    return Err(AppError::BadRequest("Event label is required".to_string()));
                }
            }
            AiToolConfiguration::CodeRunner(config) => {
                if config.code.trim().is_empty() {
                    return Err(AppError::BadRequest(
                        "Code runner source is required".to_string(),
                    ));
                }
                validate_code_runner_env_variables(config.env_variables.as_deref())?;
            }
            AiToolConfiguration::Internal(_) => {
                // Internal tools don't need validation - they're system-defined
            }
            AiToolConfiguration::Mcp(_) => {}
        }

        Ok(())
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<AiTool, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        self.validate().await?;
        let now = Utc::now();
        let tool_id = self.id;
        let tool_type_str: String = self.tool_type.into();
        let configuration_json = serde_json::to_value(&self.configuration)
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        let tool = sqlx::query!(
            r#"
            INSERT INTO ai_tools (id, created_at, updated_at, name, description, tool_type, deployment_id, requires_user_approval, configuration)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING id, created_at, updated_at, name, description, tool_type, deployment_id, requires_user_approval, configuration
            "#,
            tool_id,
            now,
            now,
            self.name,
            self.description,
            tool_type_str,
            self.deployment_id,
            self.requires_user_approval,
            configuration_json,
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        let tool_type = AiToolType::from(tool.tool_type);
        let configuration = serde_json::from_value(tool.configuration)
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        Ok(AiTool {
            id: tool.id,
            created_at: tool.created_at,
            updated_at: tool.updated_at,
            name: tool.name,
            description: tool.description,
            tool_type,
            deployment_id: tool.deployment_id,
            requires_user_approval: tool.requires_user_approval,
            configuration,
        })
    }
}

pub struct UpdateAiToolCommand {
    pub deployment_id: i64,
    pub tool_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub tool_type: Option<AiToolType>,
    pub requires_user_approval: Option<bool>,
    pub configuration: Option<AiToolConfiguration>,
}

impl UpdateAiToolCommand {
    pub fn new(deployment_id: i64, tool_id: i64) -> Self {
        Self {
            deployment_id,
            tool_id,
            name: None,
            description: None,
            tool_type: None,
            requires_user_approval: None,
            configuration: None,
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

    pub fn with_tool_type(mut self, tool_type: AiToolType) -> Self {
        self.tool_type = Some(tool_type);
        self
    }

    pub fn with_requires_user_approval(mut self, requires_user_approval: bool) -> Self {
        self.requires_user_approval = Some(requires_user_approval);
        self
    }

    pub fn with_configuration(mut self, configuration: AiToolConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }

    async fn validate(&self) -> Result<(), AppError> {
        // Basic validation
        if let Some(name) = &self.name {
            if name.trim().is_empty() {
                return Err(AppError::BadRequest("Tool name is required".to_string()));
            }
        }

        // Type-specific validation
        if let Some(configuration) = &self.configuration {
            match configuration {
                AiToolConfiguration::Api(config) => {
                    if config.endpoint.trim().is_empty() {
                        return Err(AppError::BadRequest("API endpoint is required".to_string()));
                    }
                    if !config.endpoint.starts_with("http://")
                        && !config.endpoint.starts_with("https://")
                    {
                        return Err(AppError::BadRequest(
                            "API endpoint must be a valid URL (http:// or https://)".to_string(),
                        ));
                    }
                }
                AiToolConfiguration::PlatformEvent(config) => {
                    if config.event_label.trim().is_empty() {
                        return Err(AppError::BadRequest("Event label is required".to_string()));
                    }
                }
                AiToolConfiguration::CodeRunner(config) => {
                    if config.code.trim().is_empty() {
                        return Err(AppError::BadRequest(
                            "Code runner source is required".to_string(),
                        ));
                    }
                    validate_code_runner_env_variables(config.env_variables.as_deref())?;
                }
                AiToolConfiguration::Internal(_) => {
                    // Internal tools don't need validation - they're system-defined
                }
                AiToolConfiguration::Mcp(_) => {}
            }
        }

        Ok(())
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<AiTool, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        self.validate().await?;
        let now = Utc::now();

        let mut update_set = DynamicUpdateSet::with_updated_at();
        update_set.push_if_present("name", &self.name);
        update_set.push_if_present("description", &self.description);
        update_set.push_if_present("tool_type", &self.tool_type);
        update_set.push_if_present("requires_user_approval", &self.requires_user_approval);
        update_set.push_if_present("configuration", &self.configuration);
        let (id_param, deployment_param) = update_set.where_indexes();

        let query = format!(
            r#"
            UPDATE ai_tools
            SET {}
            WHERE id = ${} AND deployment_id = ${}
            RETURNING id, created_at, updated_at, name, description, tool_type, deployment_id, requires_user_approval, configuration
            "#,
            update_set.set_clause(),
            id_param,
            deployment_param
        );

        let mut query_builder = sqlx::query(&query);
        query_builder = query_builder.bind(now);

        if let Some(name) = self.name {
            query_builder = query_builder.bind(name);
        }
        if let Some(description) = self.description {
            query_builder = query_builder.bind(description);
        }
        if let Some(tool_type) = self.tool_type {
            let tool_type_str: String = tool_type.into();
            query_builder = query_builder.bind(tool_type_str);
        }
        if let Some(requires_user_approval) = self.requires_user_approval {
            query_builder = query_builder.bind(requires_user_approval);
        }
        if let Some(configuration) = self.configuration {
            let configuration_json = serde_json::to_value(&configuration)
                .map_err(|e| AppError::Serialization(e.to_string()))?;
            query_builder = query_builder.bind(configuration_json);
        }

        query_builder = query_builder.bind(self.tool_id).bind(self.deployment_id);

        let tool = query_builder
            .fetch_one(executor)
            .await
            .map_err(AppError::Database)?;

        let tool_type = AiToolType::from(tool.get::<String, _>("tool_type"));
        let configuration = serde_json::from_value(tool.get("configuration"))
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        Ok(AiTool {
            id: tool.get("id"),
            created_at: tool.get("created_at"),
            updated_at: tool.get("updated_at"),
            name: tool.get("name"),
            description: tool.get("description"),
            tool_type,
            deployment_id: tool.get("deployment_id"),
            requires_user_approval: tool.get("requires_user_approval"),
            configuration,
        })
    }
}

fn validate_code_runner_env_variables(
    env_variables: Option<&[CodeRunnerEnvVariable]>,
) -> Result<(), AppError> {
    const RESERVED_ENV_NAMES: &[&str] = &[
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "GEMINI_API_KEY",
        "PATH",
        "LANG",
        "LC_ALL",
        "HOME",
        "TMPDIR",
    ];

    let Some(env_variables) = env_variables else {
        return Ok(());
    };

    let mut seen = std::collections::HashSet::new();

    for variable in env_variables {
        let name = variable.name.trim();
        if name.is_empty() {
            return Err(AppError::BadRequest(
                "Code runner environment variable name is required".to_string(),
            ));
        }

        let mut chars = name.chars();
        let Some(first) = chars.next() else {
            continue;
        };

        if !(first == '_' || first.is_ascii_alphabetic())
            || !chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
        {
            return Err(AppError::BadRequest(format!(
                "Invalid code runner environment variable name '{}'",
                name
            )));
        }

        if RESERVED_ENV_NAMES.contains(&name) {
            return Err(AppError::BadRequest(format!(
                "Environment variable '{}' is reserved and cannot be overridden",
                name
            )));
        }

        if !seen.insert(name.to_string()) {
            return Err(AppError::BadRequest(format!(
                "Duplicate code runner environment variable '{}'",
                name
            )));
        }
    }

    Ok(())
}

pub struct DeleteAiToolCommand {
    pub deployment_id: i64,
    pub tool_id: i64,
}

impl DeleteAiToolCommand {
    pub fn new(deployment_id: i64, tool_id: i64) -> Self {
        Self {
            deployment_id,
            tool_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            WITH deps AS (
                SELECT COALESCE(array_agg(a.name ORDER BY a.name), ARRAY[]::TEXT[]) AS agent_names
                FROM ai_agents a
                JOIN ai_agent_tools aat ON aat.agent_id = a.id
                WHERE a.deployment_id = $1
                  AND aat.tool_id = $2
                  AND aat.deployment_id = $1
            ),
            del AS (
                DELETE FROM ai_tools
                WHERE id = $2
                  AND deployment_id = $1
                  AND (SELECT cardinality(agent_names) = 0 FROM deps)
                RETURNING id
            )
            SELECT
                deps.agent_names AS "agent_names!: Vec<String>",
                EXISTS(SELECT 1 FROM del) AS "deleted!"
            FROM deps
            "#,
            self.deployment_id,
            self.tool_id
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        if !result.agent_names.is_empty() {
            return Err(AppError::BadRequest(format!(
                "Cannot delete tool. The following agents depend on it: {}. Please remove this tool from these agents first.",
                result.agent_names.join(", ")
            )));
        }

        Ok(())
    }
}
