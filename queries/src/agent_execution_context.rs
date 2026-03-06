use common::error::AppError;
use common::state::AppState;
use models::{
    AgentExecutionContext, AgentExecutionState, AgentStatusUpdate, ExecutionContextStatus,
};
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

    pub async fn execute_with_executor<'e, E>(
        &self,
        executor: E,
    ) -> Result<AgentExecutionContext, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let context = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deployment_id,
            title, context_group, system_instructions, last_activity_at, completed_at,
            execution_state, status, source, external_context_id, external_resource_metadata,
            parent_context_id, completion_summary
            FROM agent_execution_contexts
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.context_id,
            self.deployment_id
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

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
            parent_context_id: context.parent_context_id,
            completion_summary: context.completion_summary,
        })
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<AgentExecutionContext, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        self.execute_with_executor(&mut *conn).await
    }
}

impl super::Query for GetExecutionContextQuery {
    type Output = AgentExecutionContext;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}

pub struct ListExecutionContextsQuery {
    pub deployment_id: i64,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub status_filter: Option<String>,
    pub context_group_filter: Option<String>,
    pub source_filter: Option<String>,
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
            source_filter: None,
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

    pub fn with_source_filter(mut self, source: String) -> Self {
        self.source_filter = Some(source);
        self
    }

    pub fn with_title_search(mut self, title: String) -> Self {
        self.title_search = Some(title);
        self
    }

    pub async fn execute_with<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<Vec<AgentExecutionContext>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let limit = self.limit.unwrap_or(50) as i64;
        let offset = self.offset.unwrap_or(0) as i64;

        let mut query = sqlx::QueryBuilder::new(
            "SELECT id, created_at, updated_at, deployment_id,
             title, context_group, system_instructions, last_activity_at, completed_at,
             execution_state, status, source, external_context_id, external_resource_metadata,
             parent_context_id, completion_summary
             FROM agent_execution_contexts
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

        if let Some(ref context_group_filter) = self.context_group_filter {
            query.push(" AND context_group = ");
            query.push_bind(context_group_filter);
        }

        if let Some(ref source_filter) = self.source_filter {
            query.push(" AND source = ");
            query.push_bind(source_filter);
        }

        query.push(" ORDER BY last_activity_at DESC LIMIT ");
        query.push_bind(limit);
        query.push(" OFFSET ");
        query.push_bind(offset);

        let rows = query
            .build()
            .fetch_all(&mut *conn)
            .await
            .map_err(AppError::Database)?;

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
                parent_context_id: row.get("parent_context_id"),
                completion_summary: row.get("completion_summary"),
            });
        }

        Ok(result)
    }
}

impl super::Query for ListExecutionContextsQuery {
    type Output = Vec<AgentExecutionContext>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}

/// Get all child contexts spawned by a parent agent
pub struct GetChildContextsQuery {
    pub parent_context_id: i64,
    pub deployment_id: i64,
    pub include_completed: bool,
}

impl GetChildContextsQuery {
    pub fn new(parent_context_id: i64, deployment_id: i64) -> Self {
        Self {
            parent_context_id,
            deployment_id,
            include_completed: false,
        }
    }

    pub fn include_completed(mut self) -> Self {
        self.include_completed = true;
        self
    }
}

impl super::Query for GetChildContextsQuery {
    type Output = Vec<AgentExecutionContext>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Use QueryBuilder for dynamic WHERE clause
        let mut query = sqlx::QueryBuilder::new(
            r#"
            SELECT id, created_at, updated_at, deployment_id,
                   title, context_group, system_instructions, last_activity_at, completed_at,
                   execution_state, status, source, external_context_id, external_resource_metadata,
                   parent_context_id, completion_summary
            FROM agent_execution_contexts
            WHERE parent_context_id = "#,
        );
        query.push_bind(self.parent_context_id);
        query.push(" AND deployment_id = ");
        query.push_bind(self.deployment_id);

        if !self.include_completed {
            query.push(" AND status NOT IN ('completed', 'failed')");
        }

        query.push(" ORDER BY created_at ASC");

        let rows = query
            .build()
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
                parent_context_id: row.get("parent_context_id"),
                completion_summary: row.get("completion_summary"),
            });
        }

        Ok(result)
    }
}

/// Get status update timeline for a context
pub struct GetStatusUpdatesQuery {
    pub context_id: i64,
    pub limit: Option<i64>,
}

impl GetStatusUpdatesQuery {
    pub fn new(context_id: i64) -> Self {
        Self {
            context_id,
            limit: None,
        }
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }
}

impl super::Query for GetStatusUpdatesQuery {
    type Output = Vec<AgentStatusUpdate>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let limit = self.limit.unwrap_or(100);

        let rows = sqlx::query!(
            "SELECT id, context_id, status_update, metadata, created_at FROM agent_status_updates WHERE context_id = $1 ORDER BY created_at ASC LIMIT $2",
            self.context_id,
            limit
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(AgentStatusUpdate {
                id: row.id,
                context_id: row.context_id,
                status_update: row.status_update,
                metadata: row.metadata,
                created_at: row.created_at.unwrap_or_else(|| chrono::Utc::now()),
            });
        }

        Ok(result)
    }
}

/// Latest status update per context (batched lookup).
#[derive(Clone, Debug)]
pub struct LatestStatusUpdate {
    pub context_id: i64,
    pub status_update: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub struct GetLatestStatusUpdatesForContextsQuery {
    pub context_ids: Vec<i64>,
}

impl GetLatestStatusUpdatesForContextsQuery {
    pub fn new(context_ids: Vec<i64>) -> Self {
        Self { context_ids }
    }
}

impl super::Query for GetLatestStatusUpdatesForContextsQuery {
    type Output = Vec<LatestStatusUpdate>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        if self.context_ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query(
            r#"
            SELECT DISTINCT ON (context_id) context_id, status_update, created_at
            FROM agent_status_updates
            WHERE context_id = ANY($1::bigint[])
            ORDER BY context_id, created_at DESC
            "#,
        )
        .bind(&self.context_ids)
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::Database)?;

        use sqlx::Row;
        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            result.push(LatestStatusUpdate {
                context_id: row.get("context_id"),
                status_update: row.get("status_update"),
                created_at: row.get("created_at"),
            });
        }

        Ok(result)
    }
}

/// Get the parent context of a child agent
pub struct GetParentContextQuery {
    pub context_id: i64,
    pub deployment_id: i64,
}

impl GetParentContextQuery {
    pub fn new(context_id: i64, deployment_id: i64) -> Self {
        Self {
            context_id,
            deployment_id,
        }
    }
}

impl super::Query for GetParentContextQuery {
    type Output = Option<AgentExecutionContext>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // First get parent_id
        let parent_id_opt = sqlx::query_scalar!(
            "SELECT parent_context_id FROM agent_execution_contexts
             WHERE id = $1 AND deployment_id = $2",
            self.context_id,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        if let Some(parent_id) = parent_id_opt {
            let ctx = sqlx::query!(
                r#"
                SELECT id, created_at, updated_at, deployment_id,
                       title, context_group, system_instructions, last_activity_at, completed_at,
                       execution_state, status, source, external_context_id, external_resource_metadata,
                       parent_context_id, completion_summary
                FROM agent_execution_contexts
                WHERE id = $1 AND deployment_id = $2
                "#,
                parent_id,
                self.deployment_id
            )
            .fetch_one(&app_state.db_pool)
            .await?;

            let status = ExecutionContextStatus::from_str(&ctx.status).unwrap_or_default();
            let execution_state = ctx
                .execution_state
                .as_ref()
                .and_then(|s| serde_json::from_value::<AgentExecutionState>(s.clone()).ok());

            Ok(Some(AgentExecutionContext {
                id: ctx.id,
                created_at: ctx.created_at,
                updated_at: ctx.updated_at,
                deployment_id: ctx.deployment_id,
                title: ctx.title,
                context_group: ctx.context_group,
                system_instructions: ctx.system_instructions,
                last_activity_at: ctx.last_activity_at,
                completed_at: ctx.completed_at,
                execution_state,
                status,
                source: ctx.source,
                external_context_id: ctx.external_context_id,
                external_resource_metadata: ctx.external_resource_metadata,
                parent_context_id: ctx.parent_context_id,
                completion_summary: ctx.completion_summary,
            }))
        } else {
            Ok(None)
        }
    }
}

/// Get completion summary for a specific child context
pub struct GetChildCompletionSummaryQuery {
    pub child_context_id: i64,
    pub deployment_id: i64,
    pub parent_context_id: Option<i64>,
}

impl GetChildCompletionSummaryQuery {
    pub fn new(child_context_id: i64, deployment_id: i64) -> Self {
        Self {
            child_context_id,
            deployment_id,
            parent_context_id: None,
        }
    }

    pub fn with_parent_context(mut self, parent_context_id: i64) -> Self {
        self.parent_context_id = Some(parent_context_id);
        self
    }
}

impl super::Query for GetChildCompletionSummaryQuery {
    type Output = Option<serde_json::Value>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let completion_summary = if let Some(parent_context_id) = self.parent_context_id {
            let row = sqlx::query!(
                r#"
                SELECT completion_summary
                FROM agent_execution_contexts
                WHERE id = $1 AND deployment_id = $2 AND parent_context_id = $3
                "#,
                self.child_context_id,
                self.deployment_id,
                parent_context_id
            )
            .fetch_optional(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;
            row.and_then(|r| r.completion_summary)
        } else {
            let row = sqlx::query!(
                r#"
                SELECT completion_summary
                FROM agent_execution_contexts
                WHERE id = $1 AND deployment_id = $2
                "#,
                self.child_context_id,
                self.deployment_id
            )
            .fetch_optional(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;
            row.and_then(|r| r.completion_summary)
        };

        Ok(completion_summary)
    }
}

/// Get all completion summaries for a parent's children
pub struct GetChildrenCompletionSummariesQuery {
    pub parent_context_id: i64,
    pub deployment_id: i64,
}

impl GetChildrenCompletionSummariesQuery {
    pub fn new(parent_context_id: i64, deployment_id: i64) -> Self {
        Self {
            parent_context_id,
            deployment_id,
        }
    }
}

impl super::Query for GetChildrenCompletionSummariesQuery {
    type Output = Vec<ChildCompletionSummary>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let rows = sqlx::query!(
            r#"
            SELECT id, title, status, completion_summary, completed_at
            FROM agent_execution_contexts
            WHERE parent_context_id = $1 AND deployment_id = $2
              AND completion_summary IS NOT NULL
            ORDER BY completed_at DESC
            "#,
            self.parent_context_id,
            self.deployment_id
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(ChildCompletionSummary {
                context_id: row.id,
                title: row.title,
                status: row.status,
                completion_summary: row.completion_summary,
                completed_at: row.completed_at,
            });
        }

        Ok(result)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ChildCompletionSummary {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub context_id: i64,
    pub title: String,
    pub status: String,
    pub completion_summary: Option<serde_json::Value>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}
