use crate::{
    error::AppError, models::agent_dynamic_context::AgentDynamicContextSearchResult,
    queries::Query, state::AppState,
};
use pgvector::Vector;
use sqlx::Row;

pub struct SearchAgentDynamicContextQuery {
    pub execution_context_id: i64,
    pub query_embedding: Vec<f32>,
    pub limit: i64,
}

impl Query for SearchAgentDynamicContextQuery {
    type Output = Vec<AgentDynamicContextSearchResult>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let query_embedding = Vector::from(self.query_embedding.clone());

        let rows = sqlx::query(
            r#"
            SELECT id, execution_context_id, content, source, embedding, created_at,
                   (embedding <-> $1)::float8 as score
            FROM agent_dynamic_context
            WHERE execution_context_id = $2
            ORDER BY (embedding <-> $1) ASC LIMIT $3
            "#,
        )
        .bind(query_embedding)
        .bind(self.execution_context_id)
        .bind(self.limit)
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        let mut results = Vec::new();
        for row in rows {
            results.push(AgentDynamicContextSearchResult {
                id: row.try_get("id").map_err(AppError::from)?,
                execution_context_id: row.try_get("execution_context_id").map_err(AppError::from)?,
                content: row.try_get("content").map_err(AppError::from)?,
                source: row.try_get("source").map_err(AppError::from)?,
                embedding: row.try_get("embedding").map_err(AppError::from)?,
                created_at: row.try_get("created_at").map_err(AppError::from)?,
                score: row.try_get("score").map_err(AppError::from)?,
            });
        }

        Ok(results)
    }
}
