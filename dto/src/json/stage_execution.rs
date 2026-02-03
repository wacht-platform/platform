use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn deserialize_json_value<'de, D>(deserializer: D) -> Result<Value, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = String::deserialize(deserializer)?;
    if s.trim().is_empty() || s == "{}" {
        Ok(Value::Object(serde_json::Map::new()))
    } else {
        serde_json::from_str(&s)
            .map_err(|e| de::Error::custom(format!("Failed to parse JSON: {}. Input: {}", e, s)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExploration {
    pub message: String,
    pub analysis: String,
    pub approach: String,
    pub required_tools: RequiredTools,
    pub potential_issues: PotentialIssues,
    pub action_steps: ActionSteps,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequiredTools {
    pub tool: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PotentialIssues {
    pub issue: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionSteps {
    pub step: Vec<ActionStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionStep {
    pub action_type: String,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionExecution {
    pub message: String,
    pub execution: ActionExecutionDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionExecutionDetails {
    #[serde(rename = "tool_call")]
    ToolCall(ToolCallExecution),
    #[serde(rename = "context_search")]
    ContextSearch(ContextSearchExecution),
    #[serde(rename = "memory_operation")]
    MemoryOperation(MemoryOperationExecution),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallExecution {
    pub tool_name: String,
    #[serde(deserialize_with = "deserialize_json_value")]
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSearchExecution {
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryOperationExecution {
    pub operation_type: String,
    pub content: String,
    pub memory_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCorrection {
    pub message: String,
    pub analysis: String,
    pub correction_strategy: String,
    pub alternative_approach: ActionStep,
    pub retry_original: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskVerification {
    pub message: String,
    pub objective_met: bool,
    pub completion_percentage: u8,
    pub results_summary: String,
    #[serde(default)]
    pub gaps: Vec<String>,
    #[serde(default)]
    pub follow_up_tasks: Vec<FollowUpTask>,
    pub memory_updates: Option<Vec<MemoryUpdate>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowUpTask {
    pub id: String,
    pub objective: String,
    pub description: String,
    pub priority: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUpdate {
    pub content: String,
    #[serde(rename = "type")]
    pub memory_type: String,
    pub importance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStage {
    Exploration,
    Action,
    Correction,
    Verification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    pub stage: TaskStage,
    pub user_message: String,
    pub data: Value,
}
