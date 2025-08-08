use super::agent_responses::{ExecutionAction, ExecutableTask};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// DTO types for agent executor

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct StepDecision {
    pub next_step: NextStep,
    pub reasoning: String,
    pub confidence: f64,
    pub direct_execution: Option<ExecutionAction>,
    pub acknowledgment: Option<AcknowledgmentData>,
    pub planned_tasks: Option<Vec<ExecutableTask>>,
    pub examine_tool: Option<ExamineToolData>,
    pub examine_workflow: Option<ExamineWorkflowData>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ExamineToolData {
    pub tool_name: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ExamineWorkflowData {
    pub workflow_name: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AcknowledgmentData {
    pub message: String,
    pub further_action_required: bool,
    pub objective: ObjectiveDefinition,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum NextStep {
    Acknowledge,
    GatherContext,
    DirectExecution,
    TaskPlanning,
    FinishPlanning,
    ExecuteTasks,
    ValidateProgress,
    DeliverResponse,
    RequestUserInput,
    Complete,
    ExamineTool,
    ExamineWorkflow,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectiveDefinition {
    pub primary_goal: String,
    pub success_criteria: Vec<String>,
    pub constraints: Vec<String>,
    pub context_from_history: String,
    pub inferred_intent: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationInsights {
    pub is_continuation: bool,
    pub topic_evolution: String,
    pub user_preferences: Vec<String>,
    pub relevant_past_outcomes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskExecutionResult {
    pub task_id: String,
    pub status: String,
    pub output: Option<Value>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ConverseRequest {
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AcknowledgmentResponse {
    pub message: String,
    pub further_action_required: bool,
    pub reasoning: String,
    pub objective: ObjectiveDefinition,
    pub conversation_insights: ConversationInsights,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct IdeationResponse {
    pub reasoning_summary: String,
    pub needs_more_iteration: bool,
    pub context_search_request: Option<String>,
    pub requires_user_input: bool,
    pub user_input_request: Option<String>,
    pub execution_plan: ExecutionPlan,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub analysis: PlanAnalysis,
    pub success_criteria: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanAnalysis {
    pub understanding: String,
    pub approach: String,
    pub tradeoffs: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowValidationResult {
    pub ready_to_execute: bool,
    pub missing_requirements: Vec<String>,
    pub validation_message: String,
}
