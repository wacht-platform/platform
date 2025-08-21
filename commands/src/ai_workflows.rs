use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::{AiWorkflow, WorkflowConfiguration, WorkflowDefinition};

use chrono::Utc;
use sqlx::Row;

pub struct CreateAiWorkflowCommand {
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub configuration: WorkflowConfiguration,
    pub workflow_definition: WorkflowDefinition,
}

impl CreateAiWorkflowCommand {
    pub fn new(
        deployment_id: i64,
        name: String,
        description: Option<String>,
        configuration: WorkflowConfiguration,
        workflow_definition: WorkflowDefinition,
    ) -> Self {
        Self {
            deployment_id,
            name,
            description,
            configuration,
            workflow_definition,
        }
    }
}

impl Command for CreateAiWorkflowCommand {
    type Output = AiWorkflow;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let workflow_id = app_state.sf.next_id()? as i64;
        let now = Utc::now();

        let configuration_json = serde_json::to_value(&self.configuration)
            .map_err(|e| AppError::Serialization(e.to_string()))?;
        let workflow_definition_json = serde_json::to_value(&self.workflow_definition)
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        let workflow = sqlx::query!(
            r#"
            INSERT INTO ai_workflows (id, created_at, updated_at, name, description, deployment_id, configuration, workflow_definition)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, created_at, updated_at, name, description, deployment_id, configuration, workflow_definition
            "#,
            workflow_id,
            now,
            now,
            self.name,
            self.description,
            self.deployment_id,
            configuration_json,
            workflow_definition_json,
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        let configuration = serde_json::from_value(workflow.configuration)
            .map_err(|e| AppError::Serialization(e.to_string()))?;
        let workflow_definition = serde_json::from_value(workflow.workflow_definition)
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        Ok(AiWorkflow {
            id: workflow.id,
            created_at: workflow.created_at,
            updated_at: workflow.updated_at,
            name: workflow.name,
            description: workflow.description,
            deployment_id: workflow.deployment_id,
            configuration,
            workflow_definition,
        })
    }
}

pub struct UpdateAiWorkflowCommand {
    pub deployment_id: i64,
    pub workflow_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub configuration: Option<WorkflowConfiguration>,
    pub workflow_definition: Option<WorkflowDefinition>,
}

impl UpdateAiWorkflowCommand {
    pub fn new(deployment_id: i64, workflow_id: i64) -> Self {
        Self {
            deployment_id,
            workflow_id,
            name: None,
            description: None,
            configuration: None,
            workflow_definition: None,
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

    pub fn with_configuration(mut self, configuration: WorkflowConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }

    pub fn with_workflow_definition(mut self, workflow_definition: WorkflowDefinition) -> Self {
        self.workflow_definition = Some(workflow_definition);
        self
    }
}

impl Command for UpdateAiWorkflowCommand {
    type Output = AiWorkflow;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        if self.configuration.is_some() {
            query_parts.push(format!("configuration = ${}", param_count));
            param_count += 1;
        }
        if self.workflow_definition.is_some() {
            query_parts.push(format!("workflow_definition = ${}", param_count));
            param_count += 1;
        }

        let query = format!(
            r#"
            UPDATE ai_workflows
            SET {}
            WHERE id = ${} AND deployment_id = ${}
            RETURNING id, created_at, updated_at, name, description, deployment_id, configuration, workflow_definition
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
        if let Some(configuration) = self.configuration {
            let configuration_json = serde_json::to_value(&configuration)
                .map_err(|e| AppError::Serialization(e.to_string()))?;
            query_builder = query_builder.bind(configuration_json);
        }
        if let Some(workflow_definition) = self.workflow_definition {
            let workflow_definition_json = serde_json::to_value(&workflow_definition)
                .map_err(|e| AppError::Serialization(e.to_string()))?;
            query_builder = query_builder.bind(workflow_definition_json);
        }

        query_builder = query_builder
            .bind(self.workflow_id)
            .bind(self.deployment_id);

        let workflow = query_builder
            .fetch_one(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

        let configuration = serde_json::from_value(workflow.get("configuration"))
            .map_err(|e| AppError::Serialization(e.to_string()))?;
        let workflow_definition = serde_json::from_value(workflow.get("workflow_definition"))
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        Ok(AiWorkflow {
            id: workflow.get("id"),
            created_at: workflow.get("created_at"),
            updated_at: workflow.get("updated_at"),
            name: workflow.get("name"),
            description: workflow.get("description"),
            deployment_id: workflow.get("deployment_id"),
            configuration,
            workflow_definition,
        })
    }
}

pub struct DeleteAiWorkflowCommand {
    pub deployment_id: i64,
    pub workflow_id: i64,
}

impl DeleteAiWorkflowCommand {
    pub fn new(deployment_id: i64, workflow_id: i64) -> Self {
        Self {
            deployment_id,
            workflow_id,
        }
    }
}

impl Command for DeleteAiWorkflowCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Check if any agents depend on this workflow
        let dependent_agents = sqlx::query!(
            r#"
            SELECT a.id, a.name
            FROM ai_agents a
            WHERE a.deployment_id = $1
            AND a.configuration->'workflow_ids' ? $2::text
            "#,
            self.deployment_id,
            self.workflow_id.to_string()
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
                "Cannot delete workflow. The following agents depend on it: {}. Please remove this workflow from these agents first.",
                agent_names.join(", ")
            )));
        }

        let mut tx = app_state
            .db_pool
            .begin()
            .await
            .map_err(|e| AppError::Database(e))?;

        // Delete workflow executions first
        sqlx::query!(
            "DELETE FROM ai_workflow_executions WHERE workflow_id = $1",
            self.workflow_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Delete the workflow
        sqlx::query!(
            "DELETE FROM ai_workflows WHERE id = $1 AND deployment_id = $2",
            self.workflow_id,
            self.deployment_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        tx.commit().await.map_err(|e| AppError::Database(e))?;

        Ok(())
    }
}
