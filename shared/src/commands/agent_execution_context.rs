use chrono::{DateTime, Utc};
use crate::{
    commands::Command,
    error::AppError,
    models::AgentExecutionContext,
    state::AppState,
};

pub struct CreateExecutionContextCommand {
    pub deployment_id: i64,
}

impl CreateExecutionContextCommand {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Command for CreateExecutionContextCommand {
    type Output = AgentExecutionContext;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let context_id = app_state.sf.next_id()? as i64;
        let now = Utc::now();

        sqlx::query!(
            r#"
            INSERT INTO agent_execution_contexts
            (id, created_at, updated_at, deployment_id, title, current_goal, tasks, last_activity_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            context_id,
            now,
            now,
            self.deployment_id,
            "",
            "",
            &Vec::<String>::new(),
            now
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(AgentExecutionContext {
            id: context_id,
            created_at: now,
            updated_at: now,
            deployment_id: self.deployment_id,
            title: "".to_string(),
            current_goal: "".to_string(),
            tasks: Vec::new(),
            last_activity_at: now,
            completed_at: None,
        })
    }
}

pub struct UpdateExecutionContextQuery {
    pub context_id: i64,
    pub deployment_id: i64,
    pub current_goal: Option<String>,
    pub tasks: Option<Vec<String>>,
    pub completed_at: Option<Option<DateTime<Utc>>>,
}

impl UpdateExecutionContextQuery {
    pub fn new(context_id: i64, deployment_id: i64) -> Self {
        Self {
            context_id,
            deployment_id,
            current_goal: None,
            tasks: None,
            completed_at: None,
        }
    }

    pub fn with_current_goal(mut self, current_goal: String) -> Self {
        self.current_goal = Some(current_goal);
        self
    }

    pub fn with_tasks(mut self, tasks: Vec<String>) -> Self {
        self.tasks = Some(tasks);
        self
    }

    pub fn with_completion(mut self) -> Self {
        self.completed_at = Some(Some(Utc::now()));
        self
    }
}

impl super::Command for UpdateExecutionContextQuery {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();

        if let Some(ref goal) = self.current_goal {
            sqlx::query!(
                "UPDATE agent_execution_contexts SET updated_at = $1, last_activity_at = $1, current_goal = $2 WHERE id = $3 AND deployment_id = $4",
                now,
                goal,
                self.context_id,
                self.deployment_id
            )
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;
        }

        if let Some(ref tasks) = self.tasks {
            sqlx::query!(
                "UPDATE agent_execution_contexts SET updated_at = $1, last_activity_at = $1, tasks = $2 WHERE id = $3 AND deployment_id = $4",
                now,
                tasks,
                self.context_id,
                self.deployment_id
            )
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;
        }

        if let Some(completed_at) = self.completed_at {
            sqlx::query!(
                "UPDATE agent_execution_contexts SET updated_at = $1, last_activity_at = $1, completed_at = $2 WHERE id = $3 AND deployment_id = $4",
                now,
                completed_at,
                self.context_id,
                self.deployment_id
            )
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;
        }

        Ok(())
    }
}

