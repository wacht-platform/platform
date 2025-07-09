use crate::{
    commands::Command,
    error::AppError,
    models::{AiTool, AiToolConfiguration, AiToolType},
    state::AppState,
};
use chrono::Utc;
use sqlx::Row;

pub struct CreateAiToolCommand {
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub tool_type: AiToolType,
    pub configuration: AiToolConfiguration,
}

impl CreateAiToolCommand {
    pub fn new(
        deployment_id: i64,
        name: String,
        description: Option<String>,
        tool_type: AiToolType,
        configuration: AiToolConfiguration,
    ) -> Self {
        Self {
            deployment_id,
            name,
            description,
            tool_type,
            configuration,
        }
    }

    async fn validate(&self, app_state: &AppState) -> Result<(), AppError> {
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
            AiToolConfiguration::KnowledgeBase(config) => {
                if config.knowledge_base_id <= 0 {
                    return Err(AppError::BadRequest(
                        "Knowledge base selection is required".to_string(),
                    ));
                }

                // Check if knowledge base exists and belongs to the same deployment
                let kb_exists = sqlx::query!(
                    "SELECT id FROM ai_knowledge_bases WHERE id = $1 AND deployment_id = $2",
                    config.knowledge_base_id,
                    self.deployment_id
                )
                .fetch_optional(&app_state.db_pool)
                .await
                .map_err(|e| AppError::Database(e))?;

                if kb_exists.is_none() {
                    return Err(AppError::BadRequest("Selected knowledge base does not exist or does not belong to this deployment".to_string()));
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
        }

        Ok(())
    }
}

impl Command for CreateAiToolCommand {
    type Output = AiTool;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Validate the tool configuration
        self.validate(&app_state).await?;

        let tool_id = app_state.sf.next_id()? as i64;
        let now = Utc::now();
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
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

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

    async fn validate(&self, app_state: &AppState) -> Result<(), AppError> {
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
                AiToolConfiguration::KnowledgeBase(config) => {
                    if config.knowledge_base_id <= 0 {
                        return Err(AppError::BadRequest(
                            "Knowledge base selection is required".to_string(),
                        ));
                    }

                    // Check if knowledge base exists and belongs to the same deployment
                    let kb_exists = sqlx::query!(
                        "SELECT id FROM ai_knowledge_bases WHERE id = $1 AND deployment_id = $2",
                        config.knowledge_base_id,
                        self.deployment_id
                    )
                    .fetch_optional(&app_state.db_pool)
                    .await
                    .map_err(|e| AppError::Database(e))?;

                    if kb_exists.is_none() {
                        return Err(AppError::BadRequest("Selected knowledge base does not exist or does not belong to this deployment".to_string()));
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
            }
        }

        Ok(())
    }
}

impl Command for UpdateAiToolCommand {
    type Output = AiTool;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Validate the tool configuration
        self.validate(&app_state).await?;

        let now = Utc::now();

        // Build dynamic query based on provided fields
        let mut query_parts = vec!["updated_at = $1".to_string()];
        let mut param_count = 2;

        if self.name.is_some() {
            query_parts.push(format!("name = ${}", param_count));
            param_count += 1;
        }
        if self.description.is_some() {
            query_parts.push(format!("description = ${}", param_count));
            param_count += 1;
        }
        if self.tool_type.is_some() {
            query_parts.push(format!("tool_type = ${}", param_count));
            param_count += 1;
        }
        if self.configuration.is_some() {
            query_parts.push(format!("configuration = ${}", param_count));
            param_count += 1;
        }

        let query = format!(
            r#"
            UPDATE ai_tools
            SET {}
            WHERE id = ${} AND deployment_id = ${}
            RETURNING id, created_at, updated_at, name, description, tool_type, deployment_id, configuration
            "#,
            query_parts.join(", "),
            param_count,
            param_count + 1
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
            .fetch_one(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

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
}

impl Command for DeleteAiToolCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let dependent_agents = sqlx::query!(
            r#"
            SELECT a.id, a.name
            FROM ai_agents a
            WHERE a.deployment_id = $1
            AND a.configuration->'tool_ids' ? $2::text
            "#,
            self.deployment_id,
            self.tool_id.to_string()
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        if !dependent_agents.is_empty() {
            let agent_names: Vec<String> = dependent_agents
                .iter()
                .map(|agent| agent.name.clone())
                .collect();

            return Err(AppError::BadRequest(format!(
                "Cannot delete tool. The following agents depend on it: {}. Please remove this tool from these agents first.",
                agent_names.join(", ")
            )));
        }

        // Delete the tool
        sqlx::query!(
            "DELETE FROM ai_tools WHERE id = $1 AND deployment_id = $2",
            self.tool_id,
            self.deployment_id
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(())
    }
}
