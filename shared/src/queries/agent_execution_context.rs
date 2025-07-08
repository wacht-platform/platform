use crate::dto::query::SortOrder;
use crate::error::AppError;
use crate::models::{
    AgentExecutionContext, AgentExecutionContextMessage, ExecutionMessageSender,
    ExecutionMessageType,
};
use crate::state::AppState;
use pgvector::Vector;
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
            title, current_goal, tasks, last_activity_at, completed_at
            FROM agent_execution_contexts
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.context_id,
            self.deployment_id
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

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
        })
    }
}

pub struct GetExecutionMessagesQuery {
    pub execution_context_id: i64,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub message_types: Option<Vec<ExecutionMessageType>>,
    pub sort_order: SortOrder,
}

impl GetExecutionMessagesQuery {
    pub fn new(execution_context_id: i64) -> Self {
        Self {
            execution_context_id,
            limit: Some(50),
            offset: Some(0),
            sort_order: SortOrder::Desc,
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
    type Output = Vec<AgentExecutionContextMessage>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let limit = self.limit.unwrap_or(50);
        let offset = self.offset.unwrap_or(0);

        let mut result = Vec::new();

        if self.sort_order == SortOrder::Asc {
            let messages = sqlx::query!(
                r#"
                SELECT id, created_at, execution_context_id, message_type, sender, content, embedding as "embedding: Vector", extracted_data
                FROM agent_execution_messages
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

            for message in messages {
                let message_type =
                    ExecutionMessageType::from_str(&message.message_type).unwrap_or_default();
                let sender = ExecutionMessageSender::from_str(&message.sender).unwrap_or_default();

                result.push(AgentExecutionContextMessage {
                    id: message.id,
                    created_at: message.created_at,
                    execution_context_id: message.execution_context_id,
                    message_type,
                    sender,
                    content: message.content,
                    embedding: message.embedding,
                    extracted_data: message.extracted_data,
                });
            }
        } else {
            let messages = sqlx::query!(
                r#"
                SELECT id, created_at, execution_context_id, message_type, sender, content, embedding as "embedding: Vector", extracted_data
                FROM agent_execution_messages
                WHERE execution_context_id = $1
                ORDER BY created_at DESC
                LIMIT $2 OFFSET $3
                "#,
                self.execution_context_id,
                limit,
                offset
            )
            .fetch_all(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

            for message in messages {
                let message_type =
                    ExecutionMessageType::from_str(&message.message_type).unwrap_or_default();
                let sender = ExecutionMessageSender::from_str(&message.sender).unwrap_or_default();

                result.push(AgentExecutionContextMessage {
                    id: message.id,
                    created_at: message.created_at,
                    execution_context_id: message.execution_context_id,
                    message_type,
                    sender,
                    content: message.content,
                    embedding: message.embedding,
                    extracted_data: message.extracted_data,
                });
            }
        };

        Ok(result)
    }
}
