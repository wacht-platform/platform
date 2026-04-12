use common::error::AppError;
use models::{AgentThreadState, AgentThreadStatus};
use serde::de::DeserializeOwned;
use std::str::FromStr;

fn parse_optional_json<T: DeserializeOwned>(
    value: Option<serde_json::Value>,
    field: &str,
) -> Result<Option<T>, AppError> {
    value
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Failed to parse {}: {}", field, e)))
        })
        .transpose()
}

pub struct GetAgentThreadStateQuery {
    pub thread_id: i64,
    pub deployment_id: i64,
}

impl GetAgentThreadStateQuery {
    pub fn new(thread_id: i64, deployment_id: i64) -> Self {
        Self {
            thread_id,
            deployment_id,
        }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<AgentThreadState, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let thread = sqlx::query!(
            r#"
            SELECT
                id,
                created_at,
                updated_at,
                deployment_id,
                actor_id,
                project_id,
                title,
                CASE WHEN thread_purpose = 'conversation' THEN 'user_facing' ELSE 'internal' END as "thread_visibility!",
                thread_purpose,
                responsibility,
                reusable,
                accepts_assignments,
                capability_tags,
                system_instructions,
                last_activity_at,
                completed_at,
                execution_state,
                status
            FROM agent_threads
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.thread_id,
            self.deployment_id
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        let status = AgentThreadStatus::from_str(&thread.status)
            .map_err(|_| AppError::Internal(format!("Invalid thread status: {}", thread.status)))?;

        let execution_state = parse_optional_json(thread.execution_state, "execution_state")?;

        Ok(AgentThreadState {
            id: thread.id,
            created_at: thread.created_at,
            updated_at: thread.updated_at,
            deployment_id: thread.deployment_id,
            actor_id: thread.actor_id,
            project_id: thread.project_id,
            title: thread.title,
            thread_visibility: thread.thread_visibility,
            thread_purpose: thread.thread_purpose,
            responsibility: thread.responsibility,
            reusable: thread.reusable,
            accepts_assignments: thread.accepts_assignments,
            capability_tags: thread.capability_tags,
            system_instructions: thread.system_instructions,
            last_activity_at: thread.last_activity_at,
            completed_at: thread.completed_at,
            execution_state,
            status,
        })
    }
}

pub struct ListAgentThreadStatesQuery {
    pub deployment_id: i64,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub status_filter: Option<String>,
    pub title_search: Option<String>,
}

impl ListAgentThreadStatesQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            limit: None,
            offset: None,
            status_filter: None,
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

    pub fn with_title_search(mut self, title: String) -> Self {
        self.title_search = Some(title);
        self
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<AgentThreadState>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let limit = self.limit.unwrap_or(50) as i64;
        let offset = self.offset.unwrap_or(0) as i64;

        let mut query = sqlx::QueryBuilder::new(
            "SELECT id, created_at, updated_at, deployment_id,
             actor_id, project_id, title, CASE WHEN thread_purpose = 'conversation' THEN 'user_facing' ELSE 'internal' END as thread_visibility, thread_purpose, responsibility,
             reusable, accepts_assignments, capability_tags, system_instructions, last_activity_at,
             completed_at, execution_state, status
             FROM agent_threads
             WHERE deployment_id = ",
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

        query.push(" ORDER BY last_activity_at DESC LIMIT ");
        query.push_bind(limit);
        query.push(" OFFSET ");
        query.push_bind(offset);

        let rows = query
            .build()
            .fetch_all(executor)
            .await
            .map_err(AppError::Database)?;

        let mut result = Vec::new();
        for row in rows {
            use sqlx::Row;

            let status_str: String = row.get("status");
            let status = AgentThreadStatus::from_str(&status_str).map_err(|_| {
                AppError::Internal(format!("Invalid thread status: {}", status_str))
            })?;

            let execution_state: Option<serde_json::Value> = row.get("execution_state");
            let execution_state = parse_optional_json(execution_state, "execution_state")?;

            result.push(AgentThreadState {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                deployment_id: row.get("deployment_id"),
                actor_id: row.get("actor_id"),
                project_id: row.get("project_id"),
                title: row.get("title"),
                thread_visibility: row.get("thread_visibility"),
                thread_purpose: row.get("thread_purpose"),
                responsibility: row.get("responsibility"),
                reusable: row.get("reusable"),
                accepts_assignments: row.get("accepts_assignments"),
                capability_tags: row.get("capability_tags"),
                system_instructions: row.get("system_instructions"),
                last_activity_at: row.get("last_activity_at"),
                completed_at: row.get("completed_at"),
                execution_state,
                status,
            });
        }

        Ok(result)
    }
}
