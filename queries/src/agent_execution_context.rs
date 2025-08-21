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
            title, current_goal, context_group, tasks, last_activity_at, completed_at,
            execution_state, status
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
            current_goal: context.current_goal,
            context_group: context.context_group,
            tasks: context.tasks.unwrap_or_default(),
            last_activity_at: context.last_activity_at,
            completed_at: context.completed_at,
            execution_state,
            status,
        })
    }
}

pub struct ListExecutionContextsQuery {
    pub deployment_id: i64,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub status_filter: Option<String>,
    pub context_group_filter: Option<String>,
}

impl ListExecutionContextsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            limit: None,
            offset: None,
            status_filter: None,
            context_group_filter: None,
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
}

impl super::Query for ListExecutionContextsQuery {
    type Output = Vec<AgentExecutionContext>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let limit = self.limit.unwrap_or(50) as i64;
        let offset = self.offset.unwrap_or(0) as i64;

        let contexts = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deployment_id,
            title, current_goal, context_group, tasks, last_activity_at, completed_at,
            execution_state, status
            FROM agent_execution_contexts
            WHERE deployment_id = $1
            ORDER BY last_activity_at DESC
            LIMIT $2 OFFSET $3
            "#,
            self.deployment_id,
            limit,
            offset
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        let mut result = Vec::new();
        for context in contexts {
            let status = ExecutionContextStatus::from_str(&context.status).unwrap_or_default();

            // Apply filters
            if let Some(ref status_filter) = self.status_filter {
                if &context.status != status_filter {
                    continue;
                }
            }

            if let Some(ref context_group_filter) = self.context_group_filter {
                if context.context_group.as_deref() != Some(context_group_filter) {
                    continue;
                }
            }

            let execution_state = context
                .execution_state
                .as_ref()
                .and_then(|s| serde_json::from_value::<AgentExecutionState>(s.clone()).ok());

            result.push(AgentExecutionContext {
                id: context.id,
                created_at: context.created_at,
                updated_at: context.updated_at,
                deployment_id: context.deployment_id,
                title: context.title,
                current_goal: context.current_goal,
                context_group: context.context_group,
                tasks: context.tasks.unwrap_or_default(),
                last_activity_at: context.last_activity_at,
                completed_at: context.completed_at,
                execution_state,
                status,
            });
        }

        Ok(result)
    }
}
