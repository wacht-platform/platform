use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct IdeationResponse {
    pub reasoning_summary: String,
    pub needs_more_iteration: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_search_request: Option<String>,
    pub requires_user_input: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_input_request: Option<String>,
    pub execution_plan: ExecutionPlan,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ExecutionPlan {
    pub analysis: PlanAnalysis,
    pub success_criteria: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PlanAnalysis {
    pub understanding: String,
    pub approach: String,
    pub tradeoffs: String,
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
    pub strategic_synthesis: String,
    pub context_insights: Vec<String>,
    pub refined_strategy: RefinedStrategy,
    pub needs_more_context: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategic_context_request: Option<String>,
    pub requires_user_input: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_input_request: Option<String>,
    pub strategic_readiness: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct RefinedStrategy {
    pub enhanced_approach: String,
    pub strategic_priorities: Vec<String>,
    pub success_framework: String,
    pub risk_considerations: Vec<String>,
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
    pub has_unresolvable_errors: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unresolvable_error_details: Option<String>,
    pub detected_error_patterns: Vec<ErrorPattern>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ErrorPattern {
    pub error_type: String,
    pub occurrences: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_seen: Option<String>,
    pub is_recurring: bool,
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
    AbortUnresolvable,
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

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ContextSearchDerivation {
    pub search_query: String,
    pub search_scope: SearchScope,
    pub search_rationale: String,
    pub filters: ContextSearchFilters,
    pub alternative_queries: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SearchScope {
    KnowledgeBase,
    Experience,
    Universal,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ContextSearchFilters {
    pub max_results: i32,
    pub min_relevance: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boost_keywords: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_range: Option<String>,
    pub search_mode: SearchModeType,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SearchModeType {
    Semantic,
    Keyword,
    Hybrid,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct KnowledgeBaseSearchStrategy {
    pub search_strategy: String,
    pub strategy_reasoning: String,
    pub search_steps: Vec<KnowledgeBaseSearchStep>,
    pub expected_results: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct KnowledgeBaseSearchStep {
    pub step_type: String,
    pub step_description: String,
    pub parameters: KnowledgeBaseSearchParameters,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct KnowledgeBaseSearchParameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knowledge_base_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_keyword: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_ids: Option<Vec<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity_threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_chunks: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_context: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword_boost: Option<Vec<String>>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct KnowledgeBaseSearchPlan {
    pub search_approach: String,
    pub reasoning: String,
    pub primary_strategy: SearchStrategy,
    pub fallback_strategies: Vec<SearchStrategy>,
    pub success_criteria: SearchSuccessCriteria,
    pub expected_challenges: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SearchStrategy {
    pub strategy_type: String,
    pub description: String,
    pub parameters: KnowledgeBaseSearchParameters,
    pub priority: i32,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SearchSuccessCriteria {
    pub minimum_results: i32,
    pub relevance_threshold: f64,
    pub content_requirements: Vec<String>,
    pub validation_checks: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct KnowledgeBaseSearchExecution {
    pub execution_status: String,
    pub strategy_used: String,
    pub results_found: i32,
    pub quality_score: f64,
    pub execution_details: ExecutionDetails,
    pub discovered_patterns: Vec<String>,
    pub refinement_suggestions: Vec<RefinementSuggestion>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ExecutionDetails {
    pub documents_scanned: i32,
    pub chunks_analyzed: i32,
    pub search_iterations: i32,
    pub time_taken_ms: i64,
    pub challenges_encountered: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct RefinementSuggestion {
    pub suggestion_type: String,
    pub description: String,
    pub suggested_parameters: Option<KnowledgeBaseSearchParameters>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct KnowledgeBaseSearchValidation {
    pub validation_result: ValidationStatus,
    pub completeness_score: f64,
    pub relevance_assessment: RelevanceAssessment,
    pub content_gaps: Vec<ContentGap>,
    pub loop_decision: SearchLoopDecision,
    pub next_iteration_guidance: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Success,
    PartialSuccess,
    NeedsRefinement,
    Failed,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct RelevanceAssessment {
    pub overall_relevance: f64,
    pub key_findings: Vec<String>,
    pub missing_information: Vec<String>,
    pub confidence_level: ConfidenceLevel,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ContentGap {
    pub gap_type: String,
    pub description: String,
    pub suggested_search_terms: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SearchLoopDecision {
    Complete,
    RefineAndRetry,
    TryAlternativeStrategy,
    AbortInsufficient,
}