use super::agent_memory::MemoryCategory;
use super::agent_responses::ExecutionAction;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// DTO types for agent executor

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct StepDecision {
    pub next_step: NextStep,
    pub reasoning: String,
    pub confidence: f64,
    pub actions: Option<Vec<ExecutionAction>>,
    pub acknowledgment: Option<AcknowledgmentData>,
    pub context_gathering_directive: Option<ContextGatheringDirective>,
    pub memory_loading_directive: Option<MemoryLoadingDirective>,
    pub deep_reasoning_directive: Option<DeepReasoningDirective>,
    pub completion_message: Option<String>,
    #[serde(skip_deserializing, default)]
    pub thought_signature: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct DeepReasoningDirective {
    pub problem_statement: String,
    pub context_summary: String,
    pub expected_output_type: ReasoningOutputType,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ContextGatheringDirective {
    pub pattern: SearchPattern,
    pub objective: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus_areas: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_depth: Option<SearchDepth>,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SearchPattern {
    Troubleshooting,
    Implementation,
    Analysis,
    Historical,
    Exploration,
    Verification,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SearchDepth {
    Shallow,
    Moderate,
    Deep,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningOutputType {
    Analysis,  // Deep analysis of a problem
    Decision,  // Make a complex decision with tradeoffs
    Plan,      // Create a detailed plan
    Synthesis, // Synthesize multiple sources of information
    Debugging, // Debug complex issues
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct MemoryLoadingDirective {
    pub scope: MemoryScope,
    pub focus: String,
    pub categories: Vec<MemoryCategory>,
    pub depth: SearchDepth,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    CurrentSession, // Only this conversation's memories
    CrossSession,   // Agent's learned patterns across all conversations
    Universal,      // Both current + cross-session memories
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
    #[serde(rename = "loadmemory")]
    LoadMemory,
    #[serde(rename = "executeaction")]
    ExecuteAction,
    #[serde(rename = "requestuserinput")]
    RequestUserInput,
    #[serde(rename = "longthinkandreason")]
    LongThinkAndReason,
    #[serde(rename = "complete")]
    Complete,
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

/// Generic file data for any file type upload
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileData {
    pub filename: String,
    pub mime_type: String,
    pub data: String, // base64 encoded
}

#[derive(Clone, Debug)]
pub struct ConverseRequest {
    pub conversation_id: i64,
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

// Context Hints - returned by gather_context for main agent to explore
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextHints {
    pub recommended_files: Vec<RecommendedFile>,
    pub search_summary: String,
    pub search_conclusion: SearchConclusion,
    pub search_terms_used: Vec<String>,
    pub knowledge_bases_searched: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecommendedFile {
    pub path: String,
    pub document_title: String,
    pub relevance_score: f32,
    pub reason: String,
    pub sample_text: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SearchConclusion {
    FoundRelevant,
    PartialMatch,
    NothingFound,
    NeedsMoreContext,
}
