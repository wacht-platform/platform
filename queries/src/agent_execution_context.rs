use common::error::AppError;
use common::state::AppState;
use models::{AgentExecutionContext, AgentExecutionState, ExecutionContextStatus};
use std::str::FromStr;

pub struct GetExecutionContextQuery {
    pub context_id: i64,
    pub deployment_id: i64,
}

impl GetExecutionContextQuery {
    pub fn new(context_id: i64, deployment_id: i64) -> Self {
        Self {
            context_id,
            deployment_id,
        }
    }
}

impl super::Query for GetExecutionContextQuery {
    type Output = AgentExecutionContext;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let context = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deployment_id,
            title, context_group, system_instructions, last_activity_at, completed_at,
            execution_state, status, source, external_context_id, external_resource_metadata
            FROM agent_execution_contexts
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.context_id,
            self.deployment_id
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        let status = ExecutionContextStatus::from_str(&context.status).unwrap_or_default();

        let execution_state = context
            .execution_state
            .as_ref()
            .and_then(|s| serde_json::from_value::<AgentExecutionState>(s.clone()).ok());

        Ok(AgentExecutionContext {
            id: context.id,
            created_at: context.created_at,
            updated_at: context.updated_at,
            deployment_id: context.deployment_id,
            title: context.title,
            context_group: context.context_group,
            system_instructions: context.system_instructions,
            last_activity_at: context.last_activity_at,
            completed_at: context.completed_at,
            execution_state,
            status,
            source: context.source,
            external_context_id: context.external_context_id,
            external_resource_metadata: context.external_resource_metadata,
        })
    }
}

pub struct ListExecutionContextsQuery {
    pub deployment_id: i64,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub status_filter: Option<String>,
    pub context_group_filter: Option<String>,
    pub title_search: Option<String>,
}

impl ListExecutionContextsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            limit: None,
            offset: None,
            status_filter: None,
            context_group_filter: None,
            title_search: None,
        }
    }

    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_offset(mut self, offset: u32) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn with_status_filter(mut self, status: String) -> Self {
        self.status_filter = Some(status);
        self
    }

    pub fn with_context_group_filter(mut self, context_group: String) -> Self {
        self.context_group_filter = Some(context_group);
        self
    }

    pub fn with_title_search(mut self, title: String) -> Self {
        self.title_search = Some(title);
        self
    }
}

impl super::Query for ListExecutionContextsQuery {
    type Output = Vec<AgentExecutionContext>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let limit = self.limit.unwrap_or(50) as i64;
        let offset = self.offset.unwrap_or(0) as i64;

        // Use SQLx query builder for dynamic queries
        let mut query = sqlx::QueryBuilder::new(
            "SELECT id, created_at, updated_at, deployment_id, 
             title, context_group, system_instructions, last_activity_at, completed_at,
             execution_state, status, source, external_context_id, external_resource_metadata 
             FROM agent_execution_contexts 
             WHERE deployment_id = "
        );
        
        query.push_bind(self.deployment_id);
        
        if let Some(ref title_search) = self.title_search {
            query.push(" AND title ILIKE '%' || ");
            query.push_bind(title_search);
            query.push(" || '%'");
        }
        
        if let Some(ref status_filter) = self.status_filter {
            query.push(" AND status = ");
            query.push_bind(status_filter);
        }
        
        if let Some(ref context_group_filter) = self.context_group_filter {
            query.push(" AND context_group = ");
            query.push_bind(context_group_filter);
        }
        
        query.push(" ORDER BY last_activity_at DESC LIMIT ");
        query.push_bind(limit);
        query.push(" OFFSET ");
        query.push_bind(offset);
        
        let rows = query.build()
            .fetch_all(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

        let mut result = Vec::new();
        for row in rows {
            use sqlx::Row;
            
            let status_str: String = row.get("status");
            let status = ExecutionContextStatus::from_str(&status_str).unwrap_or_default();

            let execution_state: Option<serde_json::Value> = row.get("execution_state");
            let execution_state = execution_state
                .as_ref()
                .and_then(|s| serde_json::from_value::<AgentExecutionState>(s.clone()).ok());

            result.push(AgentExecutionContext {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                deployment_id: row.get("deployment_id"),
                title: row.get("title"),
                context_group: row.get("context_group"),
                system_instructions: row.get("system_instructions"),
                last_activity_at: row.get("last_activity_at"),
                completed_at: row.get("completed_at"),
                execution_state,
                status,
                source: row.get("source"),
                external_context_id: row.get("external_context_id"),
                external_resource_metadata: row.get("external_resource_metadata"),
            });
        }

        Ok(result)
    }
}
