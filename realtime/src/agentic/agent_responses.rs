use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct IdeationResponse {
    pub iteration_notes: String,
    pub needs_more_iteration: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_search_request: Option<String>,
    pub execution_plan: ExecutionPlan,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ExecutionPlan {
    pub message: String,
    pub analysis: PlanAnalysis,
    #[serde(rename = "task")]
    pub tasks: Vec<PlannedTask>,
    pub success_criteria: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PlanAnalysis {
    pub understanding: String,
    pub challenge: String,
    pub approach: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PlannedTask {
    pub id: String,
    pub objective: String,
    pub requirements: TaskRequirements,
    pub expected_output: String,
    #[serde(default)]
    pub dependencies: TaskDependencies,
    pub priority: TaskPriority,
    pub failure_strategy: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TaskRequirements {
    #[serde(rename = "requirement")]
    pub requirements: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct TaskDependencies {
    #[serde(rename = "dependency", default)]
    pub dependencies: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum TaskPriority {
    High,
    Medium,
    Low,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ContextGatheringResponse {
    pub understanding: String,
    pub context_findings: Vec<String>,
    pub initial_plan: InitialPlan,
    pub needs_more_context: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_context_request: Option<String>,
    pub ready_for_task_breakdown: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct InitialPlan {
    pub approach: String,
    pub main_steps: Vec<String>,
    pub success_criteria: String,
    pub potential_challenges: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ConfidenceLevel {
    High,
    Medium,
    Low,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TaskBreakdownResponse {
    pub task_breakdown: TaskBreakdownSummary,
    pub tasks: Vec<ExecutableTask>,
    pub execution_notes: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TaskBreakdownSummary {
    pub total_tasks: i32,
    pub estimated_duration: String,
    pub critical_path: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ExecutableTask {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub success_criteria: String,
    pub error_handling: String,
    pub can_run_parallel: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    ToolCall,
    WorkflowCall,
    KnowledgeSearch,
    ContextSearch,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TaskExecutionResponse {
    pub task_execution: TaskExecution,
    pub execution_status: ExecutionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_reason: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TaskExecution {
    pub approach: String,
    pub actions: ActionsList,
    pub expected_result: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ActionsList {
    #[serde(rename = "action")]
    pub actions: Vec<ExecutionAction>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ExecutionAction {
    #[serde(rename = "type")]
    pub action_type: TaskType,
    pub details: Value,
    pub purpose: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Ready,
    Blocked,
    CannotExecute,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ValidationResponse {
    pub validation_result: ValidationResult,
    pub loop_decision: LoopDecision,
    pub decision_reasoning: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_iteration_focus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_summary: Option<String>,
    pub user_message: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ValidationResult {
    pub overall_success: bool,
    pub completeness_score: f32,
    pub quality_assessment: QualityLevel,
    pub issues_found: Vec<String>,
    pub achievements: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum QualityLevel {
    Excellent,
    Good,
    Acceptable,
    NeedsImprovement,
    Poor,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum LoopDecision {
    Continue,
    Complete,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ParameterGenerationResponse {
    pub parameter_generation: ParameterGeneration,
    pub execution_notes: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ParameterGeneration {
    pub can_generate: bool,
    pub missing_information: Vec<String>,
    pub parameters: Value,
}