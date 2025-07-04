use crate::{
    commands::Command, error::AppError, models::agent_dynamic_context::AgentDynamicContext,
    state::AppState,
};
use pgvector::Vector;
use sqlx::Row;

pub struct CreateAgentDynamicContextCommand {
    pub id: i64,
    pub execution_context_id: i64,
    pub content: String,
    pub source: Option<String>,
    pub embedding: Option<Vec<f32>>,
}

impl Command for CreateAgentDynamicContextCommand {
    type Output = AgentDynamicContext;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let embedding = self.embedding.map(Vector::from);

        let row = sqlx::query(
            r#"
            INSERT INTO agent_dynamic_context (id, execution_context_id, content, source, embedding)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, execution_context_id, content, source, embedding, created_at
            "#,
        )
        .bind(self.id)
        .bind(self.execution_context_id)
        .bind(self.content)
        .bind(self.source)
        .bind(embedding)
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        let context = AgentDynamicContext {
            id: row.try_get("id").map_err(AppError::from)?,
            execution_context_id: row
                .try_get("execution_context_id")
                .map_err(AppError::from)?,
            content: row.try_get("content").map_err(AppError::from)?,
            source: row.try_get("source").map_err(AppError::from)?,
            embedding: row.try_get("embedding").map_err(AppError::from)?,
            created_at: row.try_get("created_at").map_err(AppError::from)?,
        };

        Ok(context)
    }
}
