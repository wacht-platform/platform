use crate::{
    error::AppError, models::ai_memory::MemorySearchRecord, queries::Query, state::AppState,
};
use chrono::{DateTime, Utc};
use pgvector::Vector;
use sqlx::QueryBuilder;

pub struct SearchMemoriesQuery {
    pub agent_id: i64,
    pub query_embedding: Vec<f32>,
    pub limit: i64,
    pub memory_type_filter: Vec<String>,
    pub min_importance: Option<f64>,
    pub time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
}

impl Query for SearchMemoriesQuery {
    type Output = Vec<MemorySearchRecord>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let query_embedding = Vector::from(self.query_embedding.clone());

        let max_distance = 1.2_f64;

        tracing::info!(
            "Executing memory search - Agent ID: {}, Limit: {}, Memory types: {:?}, Min importance: {:?}, Time range: {:?}, Max distance: {}",
            self.agent_id,
            self.limit,
            self.memory_type_filter,
            self.min_importance,
            self.time_range,
            max_distance
        );

        let mut query_builder: QueryBuilder<sqlx::Postgres> =
            QueryBuilder::new("SELECT *, embedding <-> ");
        query_builder.push_bind(query_embedding.clone());
        query_builder.push(" as score FROM agent_execution_memories WHERE agent_id = ");
        query_builder.push_bind(self.agent_id);

        // Add similarity threshold to filter out low-relevance results
        query_builder.push(" AND embedding <-> ");
        query_builder.push_bind(query_embedding.clone());
        query_builder.push(" <= ");
        query_builder.push_bind(max_distance);

        if !self.memory_type_filter.is_empty() {
            query_builder.push(" AND memory_type = ANY(");
            query_builder.push_bind(&self.memory_type_filter);
            query_builder.push(")");
        }

        if let Some(min_importance) = self.min_importance {
            query_builder.push(" AND importance >= ");
            query_builder.push_bind(min_importance);
        }

        if let Some((start, end)) = self.time_range {
            query_builder.push(" AND created_at >= ");
            query_builder.push_bind(start);
            query_builder.push(" AND created_at <= ");
            query_builder.push_bind(end);
        }

        query_builder.push(" ORDER BY score ASC LIMIT ");
        query_builder.push_bind(self.limit);

        let results: Vec<MemorySearchRecord> = query_builder
            .build_query_as()
            .fetch_all(&app_state.db_pool)
            .await
            .map_err(AppError::from)?;

        tracing::info!("Memory search returned {} results", results.len());

        for (idx, result) in results.iter().enumerate() {
            tracing::debug!(
                "Result {}: memory_id={}, type={}, score={:.4}, importance={:.2}, created_at={}",
                idx,
                result.id,
                result.memory_type,
                result.score,
                result.importance,
                result.created_at
            );
        }

        Ok(results)
    }
}
