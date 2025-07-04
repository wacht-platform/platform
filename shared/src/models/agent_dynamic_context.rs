use chrono::{DateTime, Utc};
use pgvector::Vector;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(FromRow, Serialize, Deserialize, Debug, Clone)]
pub struct AgentDynamicContext {
    pub id: i64,
    pub execution_context_id: i64,
    pub content: String,
    pub source: Option<String>,
    pub embedding: Option<Vector>,
    pub created_at: DateTime<Utc>,
}

#[derive(FromRow, Serialize, Deserialize, Debug, Clone)]
pub struct AgentDynamicContextSearchResult {
    pub id: i64,
    pub execution_context_id: i64,
    pub content: String,
    pub source: Option<String>,
    pub embedding: Option<Vector>,
    pub created_at: DateTime<Utc>,
    pub score: f64,
}
