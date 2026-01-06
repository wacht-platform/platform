use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use models::{AiToolConfiguration, WorkflowConfiguration, WorkflowDefinition};

#[derive(Deserialize)]
pub struct CreateAgentRequest {
    pub name: String,
    pub description: Option<String>,
    pub configuration: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct UpdateAgentRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub configuration: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct CreateToolRequest {
    pub name: String,
    pub description: Option<String>,
    pub tool_type: String,
    pub configuration: AiToolConfiguration,
}

#[derive(Deserialize)]
pub struct UpdateToolRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub tool_type: Option<String>,
    pub configuration: Option<AiToolConfiguration>,
}

#[derive(Deserialize)]
pub struct CreateWorkflowRequest {
    pub name: String,
    pub description: Option<String>,
    pub configuration: Option<WorkflowConfiguration>,
    pub workflow_definition: Option<WorkflowDefinition>,
}

#[derive(Deserialize)]
pub struct UpdateWorkflowRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub configuration: Option<WorkflowConfiguration>,
    pub workflow_definition: Option<WorkflowDefinition>,
}

#[derive(Deserialize)]
pub struct ExecuteWorkflowRequest {
    pub trigger_data: Option<serde_json::Value>,
    pub variables: Option<HashMap<String, serde_json::Value>>,
}

// AI Execution Context models
#[derive(Deserialize)]
pub struct CreateExecutionContextRequest {
    pub title: Option<String>,
    pub system_instructions: Option<String>,
    pub context_group: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateExecutionContextRequest {
    pub title: Option<String>,
    pub system_instructions: Option<String>,
    pub context_group: Option<String>,
    pub status: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum ExecuteAgentRequestType {
    #[serde(rename = "new_message")]
    NewMessage {
        message: String,
        images: Option<Vec<crate::json::agent_executor::ImageData>>,
    },
    
    #[serde(rename = "user_input_response")]
    UserInputResponse {
        message: String,
    },
    
    #[serde(rename = "platform_function_result")]
    PlatformFunctionResult {
        execution_id: String,
        result: serde_json::Value,
    },
}

#[derive(Deserialize)]
pub struct ExecuteAgentRequest {
    pub agent_name: String,
    #[serde(flatten)]
    pub execution_type: ExecuteAgentRequestType,
}

#[derive(Serialize)]
pub struct ExecuteAgentResponse {
    pub execution_id: i64,
    pub status: String,
}

#[derive(Serialize)]
pub struct UploadResult {
    pub url: String,
}
