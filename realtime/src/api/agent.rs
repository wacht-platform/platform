use crate::agentic::AgentExecutor;
use shared::error::AppError;
use shared::models::AiAgent;
use shared::state::AppState;

pub struct AgentHandler {
    pub app_state: AppState,
}

#[derive(Debug, Clone)]
pub struct ExecutionRequest {
    pub agent: AiAgent,
    pub deployment_id: i64,
    pub user_message: String,
    pub context_id: i64,
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
        let mut agent_executor = AgentExecutor::new(
            request.agent,
            request.deployment_id,
            request.context_id,
            &self.app_state,
        )
        .await?;

        let mut response_chunks = Vec::new();

        let execution_result = agent_executor
            .execute_with_streaming(&request.user_message, |chunk| {
                response_chunks.push(chunk.to_string());
                response_callback(chunk);
            })
            .await;

        match execution_result {
            Ok(_) => Ok(ExecutionResponse { response_chunks }),
            Err(_) => Ok(ExecutionResponse { response_chunks }),
        }
    }
}
