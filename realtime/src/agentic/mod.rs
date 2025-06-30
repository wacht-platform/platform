pub mod agent_executor;
pub mod context_engine;
pub mod tool_executor;
pub mod xml_parser;

pub mod memory_manager;
pub mod task_manager;
pub mod workflow_engine;

pub use agent_executor::*;
pub use tool_executor::*;
pub use xml_parser::*;

pub use memory_manager::*;
pub use task_manager::*;
pub use workflow_engine::*;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub result: Value,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentContext {
    pub agent_id: i64,
    pub deployment_id: i64,
    pub execution_context_id: i64,
    pub tools: Vec<shared::models::AiTool>,
    pub workflows: Vec<shared::models::AiWorkflow>,
    pub knowledge_bases: Vec<shared::models::AiKnowledgeBase>,
}
