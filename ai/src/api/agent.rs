use crate::agentic::AgentExecutor;
use shared::error::AppError;
use shared::models::AiExecutionContext;
use shared::state::AppState;

pub struct AgentHandler {
    pub app_state: AppState,
}

#[derive(Debug, Clone)]
pub struct ExecutionRequest {
    pub agent_name: String,
    pub deployment_id: i64,
    pub user_message: String,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExecutionResponse {
    pub response_chunks: Vec<String>,
}

impl AgentHandler {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    pub async fn execute_agent_streaming<F>(
        &self,
        request: ExecutionRequest,
        mut response_callback: F,
    ) -> Result<ExecutionResponse, AppError>
    where
        F: FnMut(&str) + Send + 'static,
    {
        let mut agent_executor =
            AgentExecutor::new(&request.agent_name, request.deployment_id, &self.app_state).await?;

        let session_id = request
            .session_id
            .unwrap_or_else(|| format!("session_{}", chrono::Utc::now().timestamp()));

        let mut response_chunks = Vec::new();

        let execution_result = agent_executor
            .execute_with_streaming(&request.user_message, &session_id, |chunk| {
                response_chunks.push(chunk.to_string());
                response_callback(chunk);
            })
            .await;

        match execution_result {
            Ok(_) => Ok(ExecutionResponse { response_chunks }),
            Err(_) => Ok(ExecutionResponse { response_chunks }),
        }
    }

    /// Get or create execution context
    pub async fn get_or_create_context(
        &self,
        agent_name: &str,
        deployment_id: i64,
        session_id: &str,
    ) -> Result<AiExecutionContext, AppError> {
        let mut agent_executor =
            AgentExecutor::new(agent_name, deployment_id, &self.app_state).await?;
        let context = agent_executor
            .create_or_get_execution_context(session_id, "")
            .await?;
        Ok(context.clone())
    }
}
