use crate::error::AppError;
use crate::models::{AgentExecutionContextMessage, ExecutionMessageSender, ExecutionMessageType};
use crate::state::AppState;
use pgvector::Vector;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageSimilarityResult {
    pub message: AgentExecutionContextMessage,
    pub similarity: f64,
}

pub struct SearchMessagesBySimilarityQuery {
    pub execution_context_id: i64,
    pub query_embedding: Vec<f32>,
    pub max_results: i64,
    pub min_similarity: f64,
    pub message_types: Vec<ExecutionMessageType>,
}

impl SearchMessagesBySimilarityQuery {
    pub fn new(execution_context_id: i64, query_embedding: Vec<f32>) -> Self {
        Self {
            execution_context_id,
            query_embedding,
            max_results: 50,
            min_similarity: 0.5,
            message_types: vec![],
        }
    }

    pub fn with_max_results(mut self, max_results: i64) -> Self {
        self.max_results = max_results;
        self
    }

    pub fn with_min_similarity(mut self, min_similarity: f64) -> Self {
        self.min_similarity = min_similarity;
        self
    }

    pub fn with_message_types(mut self, message_types: Vec<ExecutionMessageType>) -> Self {
        self.message_types = message_types;
        self
    }
}

impl super::Query for SearchMessagesBySimilarityQuery {
    type Output = Vec<MessageSimilarityResult>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let embeddings = Vector::from(self.query_embedding.clone());

        let messages = sqlx::query!(
            r#"
                SELECT 
                    m.id, m.created_at, m.execution_context_id, 
                    m.message_type, m.sender, m.content, 
                    m.embedding as "embedding: Vector", m.extracted_data,
                    1 - (m.embedding <=> $2::vector) as similarity
                FROM agent_execution_messages m
                WHERE m.execution_context_id = $1
                AND m.embedding IS NOT NULL
                AND m.message_type = ANY($3)
                AND 1 - (m.embedding <=> $2::vector) >= $4
                ORDER BY similarity DESC
                LIMIT $5
                "#,
            self.execution_context_id,
            embeddings as Vector,
            &self
                .message_types
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>(),
            self.min_similarity,
            self.max_results
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        let results = messages
            .into_iter()
            .filter_map(|row| {
                let message_type = ExecutionMessageType::from_str(&row.message_type).ok()?;
                let sender = ExecutionMessageSender::from_str(&row.sender).ok()?;
                let similarity = row.similarity?;

                Some(MessageSimilarityResult {
                    message: AgentExecutionContextMessage {
                        id: row.id,
                        created_at: row.created_at,
                        execution_context_id: row.execution_context_id,
                        message_type,
                        sender,
                        content: row.content,
                        embedding: row.embedding,
                        extracted_data: row.extracted_data,
                    },
                    similarity,
                })
            })
            .collect();

        Ok(results)
    }
}

pub struct SearchAllMessagesBySimilarityQuery {
    pub deployment_id: i64,
    pub query_embedding: Vec<f32>,
    pub max_results: i64,
    pub min_similarity: f64,
}

impl SearchAllMessagesBySimilarityQuery {
    pub fn new(deployment_id: i64, query_embedding: Vec<f32>) -> Self {
        Self {
            deployment_id,
            query_embedding,
            max_results: 100,
            min_similarity: 0.5,
        }
    }

    pub fn with_max_results(mut self, max_results: i64) -> Self {
        self.max_results = max_results;
        self
    }

    pub fn with_min_similarity(mut self, min_similarity: f64) -> Self {
        self.min_similarity = min_similarity;
        self
    }
}

impl super::Query for SearchAllMessagesBySimilarityQuery {
    type Output = Vec<MessageSimilarityResult>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let embeddings = Vector::from(self.query_embedding.clone());

        let messages = sqlx::query!(
            r#"
                SELECT 
                    m.id, m.created_at, m.execution_context_id, 
                    m.message_type, m.sender, m.content, 
                    m.embedding as "embedding: Vector", m.extracted_data,
                    1 - (m.embedding <=> $2::vector) as similarity
                FROM agent_execution_messages m
                JOIN agent_execution_contexts c ON m.execution_context_id = c.id
                WHERE c.deployment_id = $1
                AND m.embedding IS NOT NULL
                AND 1 - (m.embedding <=> $2::vector) >= $3
                ORDER BY similarity DESC
                LIMIT $4
                "#,
            self.deployment_id,
            embeddings as Vector,
            self.min_similarity,
            self.max_results
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        let results = messages
            .into_iter()
            .filter_map(|row| {
                let message_type = ExecutionMessageType::from_str(&row.message_type).ok()?;
                let sender = ExecutionMessageSender::from_str(&row.sender).ok()?;
                let similarity = row.similarity?;

                Some(MessageSimilarityResult {
                    message: AgentExecutionContextMessage {
                        id: row.id,
                        created_at: row.created_at,
                        execution_context_id: row.execution_context_id,
                        message_type,
                        sender,
                        content: row.content,
                        embedding: row.embedding,
                        extracted_data: row.extracted_data,
                    },
                    similarity,
                })
            })
            .collect();

        Ok(results)
    }
}
