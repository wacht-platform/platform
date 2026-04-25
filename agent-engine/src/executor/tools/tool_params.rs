use common::error::AppError;
use dto::json::agent_executor::ToolCallRequest;
use models::AiTool;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const MAX_LOADED_EXTERNAL_TOOLS: usize = 30;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PlannedToolCall {
    pub(crate) request: ToolCallRequest,
    pub(crate) retryable_on_failure: bool,
}

impl PlannedToolCall {
    pub(crate) fn tool_name(&self) -> &str {
        self.request.tool_name()
    }

    pub(crate) fn input_value(&self) -> Result<Value, AppError> {
        self.request
            .input_value()
            .map_err(|e| AppError::Internal(format!("Failed to serialize tool input: {e}")))
    }
}

#[derive(Clone)]
pub(crate) struct ResolvedToolCall {
    pub(crate) request: PlannedToolCall,
    pub(crate) tool: AiTool,
}

pub(crate) struct ToolExecutionLoopOutcome {
    pub any_pending: bool,
}
