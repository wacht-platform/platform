use crate::error::AppError;
use crate::models::{AiExecutionContext, AiExecutionMessage, ExecutionContextStatus, ExecutionMessageType, ExecutionMessageSender};
use crate::state::AppState;
use chrono::{DateTime, Utc};
use serde_json::Value;

pub struct CreateExecutionContextQuery {
    pub agent_id: i64,
    pub deployment_id: i64,
    pub session_id: String,
    pub title: String,
    pub current_goal: String,
    pub memory: Value,
    pub tasks: Vec<String>,
}

impl CreateExecutionContextQuery {
    pub fn new(
        agent_id: i64,
        deployment_id: i64,
        session_id: String,
        title: String,
        current_goal: String,
    ) -> Self {
        Self {
            agent_id,
            deployment_id,
            session_id,
            title,
            current_goal,
            memory: serde_json::json!({}),
            tasks: Vec::new(),
        }
    }
}

impl super::Query for CreateExecutionContextQuery {
    type Output = AiExecutionContext;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let context_id = app_state.sf.next_id()? as i64;
        let now = Utc::now();

        sqlx::query!(
            r#"
            INSERT INTO ai_execution_contexts 
            (id, created_at, updated_at, agent_id, deployment_id, session_id, title, current_goal, status, memory, tasks, last_activity_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
            context_id,
            now,
            now,
            self.agent_id,
            self.deployment_id,
            self.session_id,
            self.title,
            self.current_goal,
            serde_json::to_string(&ExecutionContextStatus::Running).unwrap_or_default(),
            self.memory,
            &self.tasks,
            now
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(AiExecutionContext {
            id: context_id,
            created_at: now,
            updated_at: now,
            agent_id: self.agent_id,
            deployment_id: self.deployment_id,
            session_id: self.session_id.clone(),
            title: self.title.clone(),
            current_goal: self.current_goal.clone(),
            status: ExecutionContextStatus::Running,
            memory: self.memory.clone(),
            tasks: self.tasks.clone(),
            last_activity_at: now,
            completed_at: None,
        })
    }
}

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
    type Output = AiExecutionContext;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let context = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, agent_id, deployment_id, session_id, 
                   title, current_goal, status, memory, tasks, last_activity_at, completed_at
            FROM ai_execution_contexts
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.context_id,
            self.deployment_id
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        let status: ExecutionContextStatus = serde_json::from_str(&context.status)
            .unwrap_or(ExecutionContextStatus::Idle);

        Ok(AiExecutionContext {
            id: context.id,
            created_at: context.created_at,
            updated_at: context.updated_at,
            agent_id: context.agent_id,
            deployment_id: context.deployment_id,
            session_id: context.session_id,
            title: context.title,
            current_goal: context.current_goal,
            status,
            memory: context.memory,
            tasks: context.tasks.unwrap_or_default(),
            last_activity_at: context.last_activity_at,
            completed_at: context.completed_at,
        })
    }
}

pub struct GetExecutionContextsBySessionQuery {
    pub session_id: String,
    pub deployment_id: i64,
    pub limit: Option<i64>,
}

impl GetExecutionContextsBySessionQuery {
    pub fn new(session_id: String, deployment_id: i64) -> Self {
        Self {
            session_id,
            deployment_id,
            limit: Some(10),
        }
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }
}

impl super::Query for GetExecutionContextsBySessionQuery {
    type Output = Vec<AiExecutionContext>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let limit = self.limit.unwrap_or(10);
        
        let contexts = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, agent_id, deployment_id, session_id, 
                   title, current_goal, status, memory, tasks, last_activity_at, completed_at
            FROM ai_execution_contexts
            WHERE session_id = $1 AND deployment_id = $2
            ORDER BY last_activity_at DESC
            LIMIT $3
            "#,
            self.session_id,
            self.deployment_id,
            limit
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        let mut result = Vec::new();
        for context in contexts {
            let status: ExecutionContextStatus = serde_json::from_str(&context.status)
                .unwrap_or(ExecutionContextStatus::Idle);

            result.push(AiExecutionContext {
                id: context.id,
                created_at: context.created_at,
                updated_at: context.updated_at,
                agent_id: context.agent_id,
                deployment_id: context.deployment_id,
                session_id: context.session_id,
                title: context.title,
                current_goal: context.current_goal,
                status,
                memory: context.memory,
                tasks: context.tasks.unwrap_or_default(),
                last_activity_at: context.last_activity_at,
                completed_at: context.completed_at,
            });
        }

        Ok(result)
    }
}

pub struct GetExecutionContextsByAgentQuery {
    pub agent_id: i64,
    pub deployment_id: i64,
    pub limit: Option<i64>,
}

impl GetExecutionContextsByAgentQuery {
    pub fn new(agent_id: i64, deployment_id: i64) -> Self {
        Self {
            agent_id,
            deployment_id,
            limit: Some(10),
        }
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }
}

impl super::Query for GetExecutionContextsByAgentQuery {
    type Output = Vec<AiExecutionContext>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let limit = self.limit.unwrap_or(10);

        let contexts = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, agent_id, deployment_id, session_id,
                   title, current_goal, status, memory, tasks, last_activity_at, completed_at
            FROM ai_execution_contexts
            WHERE agent_id = $1 AND deployment_id = $2
            ORDER BY last_activity_at DESC
            LIMIT $3
            "#,
            self.agent_id,
            self.deployment_id,
            limit
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        let mut result = Vec::new();
        for context in contexts {
            let status: ExecutionContextStatus = serde_json::from_str(&context.status)
                .unwrap_or(ExecutionContextStatus::Idle);

            result.push(AiExecutionContext {
                id: context.id,
                created_at: context.created_at,
                updated_at: context.updated_at,
                agent_id: context.agent_id,
                deployment_id: context.deployment_id,
                session_id: context.session_id,
                title: context.title,
                current_goal: context.current_goal,
                status,
                memory: context.memory,
                tasks: context.tasks.unwrap_or_default(),
                last_activity_at: context.last_activity_at,
                completed_at: context.completed_at,
            });
        }

        Ok(result)
    }
}

pub struct UpdateExecutionContextQuery {
    pub context_id: i64,
    pub deployment_id: i64,
    pub current_goal: Option<String>,
    pub status: Option<ExecutionContextStatus>,
    pub memory: Option<Value>,
    pub tasks: Option<Vec<String>>,
    pub completed_at: Option<Option<DateTime<Utc>>>,
}

impl UpdateExecutionContextQuery {
    pub fn new(context_id: i64, deployment_id: i64) -> Self {
        Self {
            context_id,
            deployment_id,
            current_goal: None,
            status: None,
            memory: None,
            tasks: None,
            completed_at: None,
        }
    }

    pub fn with_status(mut self, status: ExecutionContextStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_memory(mut self, memory: Value) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn with_tasks(mut self, tasks: Vec<String>) -> Self {
        self.tasks = Some(tasks);
        self
    }

    pub fn with_completion(mut self) -> Self {
        self.completed_at = Some(Some(Utc::now()));
        self.status = Some(ExecutionContextStatus::Completed);
        self
    }
}

impl super::Query for UpdateExecutionContextQuery {
    type Output = ();

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();

        // Use individual updates for simplicity and to avoid lifetime issues
        if let Some(ref status) = self.status {
            sqlx::query!(
                "UPDATE ai_execution_contexts SET updated_at = $1, last_activity_at = $1, status = $2 WHERE id = $3 AND deployment_id = $4",
                now,
                serde_json::to_string(status).unwrap_or_default(),
                self.context_id,
                self.deployment_id
            )
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;
        }

        if let Some(ref goal) = self.current_goal {
            sqlx::query!(
                "UPDATE ai_execution_contexts SET updated_at = $1, last_activity_at = $1, current_goal = $2 WHERE id = $3 AND deployment_id = $4",
                now,
                goal,
                self.context_id,
                self.deployment_id
            )
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;
        }

        if let Some(ref memory) = self.memory {
            sqlx::query!(
                "UPDATE ai_execution_contexts SET updated_at = $1, last_activity_at = $1, memory = $2 WHERE id = $3 AND deployment_id = $4",
                now,
                memory,
                self.context_id,
                self.deployment_id
            )
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;
        }

        if let Some(ref tasks) = self.tasks {
            sqlx::query!(
                "UPDATE ai_execution_contexts SET updated_at = $1, last_activity_at = $1, tasks = $2 WHERE id = $3 AND deployment_id = $4",
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
                "UPDATE ai_execution_contexts SET updated_at = $1, last_activity_at = $1, completed_at = $2 WHERE id = $3 AND deployment_id = $4",
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

pub struct CreateExecutionMessageQuery {
    pub execution_context_id: i64,
    pub message_type: ExecutionMessageType,
    pub sender: ExecutionMessageSender,
    pub content: String,
    pub metadata: Value,
    pub tool_calls: Option<Value>,
    pub tool_results: Option<Value>,
}

impl CreateExecutionMessageQuery {
    pub fn new(
        execution_context_id: i64,
        message_type: ExecutionMessageType,
        sender: ExecutionMessageSender,
        content: String,
    ) -> Self {
        Self {
            execution_context_id,
            message_type,
            sender,
            content,
            metadata: serde_json::json!({}),
            tool_calls: None,
            tool_results: None,
        }
    }

    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_tool_calls(mut self, tool_calls: Value) -> Self {
        self.tool_calls = Some(tool_calls);
        self
    }

    pub fn with_tool_results(mut self, tool_results: Value) -> Self {
        self.tool_results = Some(tool_results);
        self
    }
}

impl super::Query for CreateExecutionMessageQuery {
    type Output = AiExecutionMessage;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let message_id = app_state.sf.next_id()? as i64;
        let now = Utc::now();

        sqlx::query!(
            r#"
            INSERT INTO ai_execution_messages
            (id, created_at, execution_context_id, message_type, sender, content, metadata, tool_calls, tool_results)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
            message_id,
            now,
            self.execution_context_id,
            serde_json::to_string(&self.message_type).unwrap_or_default(),
            serde_json::to_string(&self.sender).unwrap_or_default(),
            self.content,
            self.metadata,
            self.tool_calls,
            self.tool_results
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(AiExecutionMessage {
            id: message_id,
            created_at: now,
            execution_context_id: self.execution_context_id,
            message_type: self.message_type.clone(),
            sender: self.sender.clone(),
            content: self.content.clone(),
            metadata: self.metadata.clone(),
            tool_calls: self.tool_calls.clone(),
            tool_results: self.tool_results.clone(),
        })
    }
}

pub struct GetExecutionMessagesQuery {
    pub execution_context_id: i64,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub message_types: Option<Vec<ExecutionMessageType>>,
}

impl GetExecutionMessagesQuery {
    pub fn new(execution_context_id: i64) -> Self {
        Self {
            execution_context_id,
            limit: Some(50),
            offset: Some(0),
            message_types: None,
        }
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_offset(mut self, offset: i64) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn with_message_types(mut self, message_types: Vec<ExecutionMessageType>) -> Self {
        self.message_types = Some(message_types);
        self
    }
}

impl super::Query for GetExecutionMessagesQuery {
    type Output = Vec<AiExecutionMessage>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let limit = self.limit.unwrap_or(50);
        let offset = self.offset.unwrap_or(0);

        let messages = sqlx::query!(
            r#"
            SELECT id, created_at, execution_context_id, message_type, sender, content, metadata, tool_calls, tool_results
            FROM ai_execution_messages
            WHERE execution_context_id = $1
            ORDER BY created_at ASC
            LIMIT $2 OFFSET $3
            "#,
            self.execution_context_id,
            limit,
            offset
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        let mut result = Vec::new();
        for message in messages {
            let message_type: ExecutionMessageType = serde_json::from_str(&message.message_type)
                .unwrap_or(ExecutionMessageType::SystemMessage);
            let sender: ExecutionMessageSender = serde_json::from_str(&message.sender)
                .unwrap_or(ExecutionMessageSender::System);

            result.push(AiExecutionMessage {
                id: message.id,
                created_at: message.created_at,
                execution_context_id: message.execution_context_id,
                message_type,
                sender,
                content: message.content,
                metadata: message.metadata,
                tool_calls: message.tool_calls,
                tool_results: message.tool_results,
            });
        }

        Ok(result)
    }
}
