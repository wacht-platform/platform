use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use models::{AiToolConfiguration, WorkflowConfiguration, WorkflowDefinition};

// AI Agent models
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

// AI Tool models
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

// AI Workflow models
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
    // Empty struct - deployment_id comes from path
}

#[derive(Serialize)]
pub struct UploadResult {
    pub url: String,
}
