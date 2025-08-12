use crate::{error::AppError, state::AppState};

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Not found")]
    NotFound,
    #[error("Bad request: {0}")]
    BadRequest(String),
}

impl From<QueryError> for AppError {
    fn from(error: QueryError) -> Self {
        match error {
            QueryError::DatabaseError(msg) => AppError::Internal(msg),
            QueryError::NotFound => AppError::NotFound("Resource not found".to_string()),
            QueryError::BadRequest(msg) => AppError::BadRequest(msg),
        }
    }
}

pub trait Query {
    type Output;

    fn execute(
        &self,
        app_state: &AppState,
    ) -> impl std::future::Future<Output = Result<Self::Output, AppError>> + Send;
}

pub mod b2b;
pub mod deployment;
pub mod invitation;
pub mod organization;
pub mod project;
pub mod signin;
pub mod user;
pub mod workspace;

pub mod agent_execution_context;
pub mod ai_agent;
pub mod ai_knowledge_base;
pub mod ai_tool;
pub mod ai_workflow;
pub mod hybrid_search;
pub use agent_execution_context::*;
pub use ai_agent::*;
pub use ai_knowledge_base::*;
pub use b2b::*;
pub use deployment::*;
pub use invitation::*;
pub use organization::*;
pub use project::*;
pub use signin::*;
pub use user::*;
pub use workspace::*;
pub mod agent_memory;
pub mod webhook;
pub mod webhook_analytics;
pub mod api_key;
pub mod notification;
pub use ai_tool::*;
pub use ai_workflow::*;
pub use hybrid_search::*;
pub use agent_memory::*;
pub use webhook::*;
pub use webhook_analytics::*;
