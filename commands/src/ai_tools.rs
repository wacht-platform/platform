use crate::dynamic_update_set::DynamicUpdateSet;
use common::error::AppError;
use models::{AiTool, AiToolConfiguration, AiToolType};

use chrono::Utc;
use sqlx::Row;

pub struct CreateAiToolCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub tool_type: AiToolType,
    pub configuration: AiToolConfiguration,
}

impl CreateAiToolCommand {
    pub fn new(
        id: i64,
        deployment_id: i64,
        name: String,
        description: Option<String>,
        tool_type: AiToolType,
        configuration: AiToolConfiguration,
    ) -> Self {
        Self {
            id,
            deployment_id,
            name,
            description,
            tool_type,
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
            AiToolConfiguration::PlatformFunction(config) => {
                if config.function_name.trim().is_empty() {
                    return Err(AppError::BadRequest(
                        "Function name is required".to_string(),
                    ));
                }
            }
            AiToolConfiguration::Internal(_) => {
                // Internal tools don't need validation - they're system-defined
            }
            AiToolConfiguration::UseExternalService(_) => {
                // External service tools don't need validation - they're system-defined
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
        let tool_id = self.id;
        let tool_type_str: String = self.tool_type.into();
        let configuration_json = serde_json::to_value(&self.configuration)
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        let tool = sqlx::query!(
            r#"
            INSERT INTO ai_tools (id, created_at, updated_at, name, description, tool_type, deployment_id, configuration)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, created_at, updated_at, name, description, tool_type, deployment_id, configuration
            "#,
            tool_id,
            now,
            now,
            self.name,
            self.description,
            tool_type_str,
            self.deployment_id,
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
                AiToolConfiguration::PlatformFunction(config) => {
                    if config.function_name.trim().is_empty() {
                        return Err(AppError::BadRequest(
                            "Function name is required".to_string(),
                        ));
                    }
                }
                AiToolConfiguration::Internal(_) => {
                    // Internal tools don't need validation - they're system-defined
                }
                AiToolConfiguration::UseExternalService(_) => {
                    // External service tools don't need validation - they're system-defined
                }
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
        update_set.push_if_present("configuration", &self.configuration);
        let (id_param, deployment_param) = update_set.where_indexes();

        let query = format!(
            r#"
            UPDATE ai_tools
            SET {}
            WHERE id = ${} AND deployment_id = ${}
            RETURNING id, created_at, updated_at, name, description, tool_type, deployment_id, configuration
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
            configuration,
        })
    }
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
