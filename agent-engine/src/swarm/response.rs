use common::error::AppError;
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
pub struct ToolSuccessResponse<T: Serialize> {
    pub success: bool,
    pub tool: String,
    #[serde(flatten)]
    pub data: T,
}

pub fn success<T: Serialize>(tool: &str, data: T) -> Result<Value, AppError> {
    serde_json::to_value(ToolSuccessResponse {
        success: true,
        tool: tool.to_string(),
        data,
    })
    .map_err(|e| AppError::Internal(format!("Failed to serialize swarm response: {}", e)))
}
