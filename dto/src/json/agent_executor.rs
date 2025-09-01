use super::agent_responses::ExecutionAction;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// DTO types for agent executor

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct StepDecision {
    pub next_step: NextStep,
    pub reasoning: String,
    pub confidence: f64,
    pub execute_action: Option<ExecutionAction>,
    pub acknowledgment: Option<AcknowledgmentData>,
    pub examine_tool: Option<ExamineToolData>,
    pub examine_workflow: Option<ExamineWorkflowData>,
    pub context_gathering_objective: Option<String>,
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
pub enum NextStep {
    #[serde(rename = "acknowledge")]
    Acknowledge,
    #[serde(rename = "gathercontext")]
    GatherContext,
    #[serde(rename = "executeaction")]
    ExecuteAction,
    #[serde(rename = "validateprogress")]
    ValidateProgress,
    #[serde(rename = "deliverresponse")]
    DeliverResponse,
    #[serde(rename = "requestuserinput")]
    RequestUserInput,
    #[serde(rename = "complete")]
    Complete,
    #[serde(rename = "examinetool")]
    ExamineTool,
    #[serde(rename = "examineworkflow")]
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageData {
    pub mime_type: String,
    pub data: String,
}

#[derive(Clone, Debug, Default)]
pub struct ConverseRequest {
    pub message: String,
    pub images: Option<Vec<ImageData>>,
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
