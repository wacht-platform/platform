use sqlx::Row;

use crate::{
    error::AppError,
    models::{AiWorkflow, AiWorkflowWithDetails, WorkflowConfiguration, WorkflowDefinition},
    queries::Query,
    state::AppState,
};

pub struct GetAiWorkflowsQuery {
    pub deployment_id: i64,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub search: Option<String>,
}

impl GetAiWorkflowsQuery {
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
}

impl Query for GetAiWorkflowsQuery {
    type Output = Vec<AiWorkflowWithDetails>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut query = r#"
            SELECT
                w.id, w.created_at, w.updated_at, w.name, w.description,
                w.deployment_id, w.configuration, w.workflow_definition,
                COALESCE(a.agents_count, 0) as agents_count,
                e.last_execution_at
            FROM ai_workflows w
            LEFT JOIN (
                SELECT workflow_id, COUNT(*) as agents_count
                FROM ai_agent_workflows
                GROUP BY workflow_id
            ) a ON w.id = a.workflow_id
            LEFT JOIN (
                SELECT workflow_id, MAX(created_at) as last_execution_at
                FROM ai_workflow_executions
                GROUP BY workflow_id
            ) e ON w.id = e.workflow_id
            WHERE w.deployment_id = $1
        "#
        .to_string();

        let mut param_count = 2;
        if self.search.is_some() {
            query.push_str(&format!(
                " AND (w.name ILIKE ${} OR w.description ILIKE ${})",
                param_count,
                param_count + 1
            ));
            param_count += 2;
        }

        query.push_str(" ORDER BY w.created_at DESC");
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

        let workflows = query_builder
            .fetch_all(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

        Ok(workflows
            .into_iter()
            .map(|row| {
                let configuration: WorkflowConfiguration =
                    serde_json::from_value(row.get("configuration")).unwrap_or_default();
                let workflow_definition: WorkflowDefinition =
                    serde_json::from_value(row.get("workflow_definition")).unwrap_or_default();

                AiWorkflowWithDetails {
                    id: row.get("id"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    name: row.get("name"),
                    description: row.get("description"),
                    deployment_id: row.get("deployment_id"),
                    configuration,
                    workflow_definition,
                    agents_count: row.get::<Option<i64>, _>("agents_count").unwrap_or(0),
                    last_execution_at: row.get("last_execution_at"),
                }
            })
            .collect())
    }
}

pub struct GetAiWorkflowByIdQuery {
    pub deployment_id: i64,
    pub workflow_id: i64,
}

impl GetAiWorkflowByIdQuery {
    pub fn new(deployment_id: i64, workflow_id: i64) -> Self {
        Self {
            deployment_id,
            workflow_id,
        }
    }
}

impl Query for GetAiWorkflowByIdQuery {
    type Output = AiWorkflowWithDetails;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let workflow = sqlx::query!(
            r#"
            SELECT
                w.id, w.created_at, w.updated_at, w.name, w.description,
                w.deployment_id, w.configuration, w.workflow_definition,
                COALESCE(a.agents_count, 0) as agents_count,
                e.last_execution_at
            FROM ai_workflows w
            LEFT JOIN (
                SELECT workflow_id, COUNT(*) as agents_count
                FROM ai_agent_workflows
                GROUP BY workflow_id
            ) a ON w.id = a.workflow_id
            LEFT JOIN (
                SELECT workflow_id, MAX(created_at) as last_execution_at
                FROM ai_workflow_executions
                GROUP BY workflow_id
            ) e ON w.id = e.workflow_id
            WHERE w.id = $1 AND w.deployment_id = $2
            "#,
            self.workflow_id,
            self.deployment_id
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        let configuration: WorkflowConfiguration =
            serde_json::from_value(workflow.configuration).unwrap_or_default();
        let workflow_definition: WorkflowDefinition =
            serde_json::from_value(workflow.workflow_definition).unwrap_or_default();

        Ok(AiWorkflowWithDetails {
            id: workflow.id,
            created_at: workflow.created_at,
            updated_at: workflow.updated_at,
            name: workflow.name,
            description: workflow.description,
            deployment_id: workflow.deployment_id,
            configuration,
            workflow_definition,
            agents_count: workflow.agents_count.unwrap_or(0),
            last_execution_at: workflow.last_execution_at,
        })
    }
}

pub struct GetAiWorkflowsByIdsQuery {
    pub deployment_id: i64,
    pub workflow_ids: Vec<i64>,
}

impl GetAiWorkflowsByIdsQuery {
    pub fn new(deployment_id: i64, workflow_ids: Vec<i64>) -> Self {
        Self {
            deployment_id,
            workflow_ids,
        }
    }
}

impl Query for GetAiWorkflowsByIdsQuery {
    type Output = Vec<AiWorkflow>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        if self.workflow_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = (1..=self.workflow_ids.len())
            .map(|i| format!("${}", i + 1))
            .collect::<Vec<_>>()
            .join(",");

        let query = format!(
            "SELECT id, created_at, updated_at, name, description, deployment_id, configuration, workflow_definition
             FROM ai_workflows
             WHERE deployment_id = $1 AND id IN ({})",
            placeholders
        );

        let mut query_builder = sqlx::query(&query);
        query_builder = query_builder.bind(self.deployment_id);

        for workflow_id in &self.workflow_ids {
            query_builder = query_builder.bind(workflow_id);
        }

        let workflows = query_builder
            .fetch_all(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

        Ok(workflows
            .into_iter()
            .map(|row| {
                let configuration: WorkflowConfiguration =
                    serde_json::from_value(row.get("configuration")).unwrap_or_default();
                let workflow_definition: WorkflowDefinition =
                    serde_json::from_value(row.get("workflow_definition")).unwrap_or_default();

                AiWorkflow {
                    id: row.get("id"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    name: row.get("name"),
                    description: row.get("description"),
                    deployment_id: row.get("deployment_id"),
                    configuration,
                    workflow_definition,
                }
            })
            .collect())
    }
}
