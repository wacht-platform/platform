use common::error::AppError;
use models::{AgentExecutionContext, ExecutionContextStatus, AgentExecutionState};
use common::state::AppState;
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
            title, current_goal, tasks, last_activity_at, completed_at,
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

        let execution_state = context.execution_state
            .as_ref()
            .and_then(|s| serde_json::from_value::<AgentExecutionState>(s.clone()).ok());

        Ok(AgentExecutionContext {
            id: context.id,
            created_at: context.created_at,
            updated_at: context.updated_at,
            deployment_id: context.deployment_id,
            title: context.title,
            current_goal: context.current_goal,
            tasks: context.tasks.unwrap_or_default(),
            last_activity_at: context.last_activity_at,
            completed_at: context.completed_at,
            execution_state,
            status,
        })
    }
}

