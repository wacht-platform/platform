pub use super::tool_calls::*;
use serde::{Deserialize, Serialize};

// DTO types for agent executor

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct NextStepDecision {
    pub next_step: NextStep,
    pub reasoning: String,
    pub confidence: f64,
    pub steer: Option<SteerData>,
    pub search_tools_directive: Option<SearchToolsDirective>,
    pub load_tools_directive: Option<LoadToolsDirective>,
    pub startaction_directive: Option<StartActionDirective>,
    pub continueaction_directive: Option<ContinueActionDirective>,
    pub abort_directive: Option<AbortDirective>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct StartActionDirective {
    pub objective: String,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub tool_call_brief: Option<ToolCallBrief>,
}

pub type ExecuteActionDirective = StartActionDirective;

#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq, Eq)]
pub struct ToolCallBrief {
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub focus_points: Vec<String>,
    #[serde(default)]
    pub tool_parameter_briefs: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct ContinueActionDirective {
    pub guidance: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AbortDirective {
    pub outcome: AbortOutcome,
    pub reason: String,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AbortOutcome {
    Blocked,
    ReturnToCoordinator,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum LocalKnowledgeSearchType {
    Semantic,
    Keyword,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemorySearchApproach {
    Semantic,
    FullText,
    Hybrid,
}

impl Default for MemorySearchApproach {
    fn default() -> Self {
        Self::Semantic
    }
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

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    Thread,
    Project,
    Actor,
    Agent,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SteerData {
    pub message: String,
    pub further_actions_required: bool,
    pub attachments: Option<Vec<ResponseAttachment>>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ResponseAttachment {
    pub path: String,
    #[serde(rename = "type")]
    pub attachment_type: ResponseAttachmentType,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ResponseAttachmentType {
    File,
    Folder,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ApprovalRequestData {
    #[serde(default)]
    pub description: String,
    pub tool_names: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SearchToolsDirective {
    pub queries: Vec<String>,
    #[serde(default)]
    pub max_results_per_query: Option<usize>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LoadToolsDirective {
    pub tool_names: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum NextStep {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "searchtools")]
    SearchTools,
    #[serde(rename = "loadtools")]
    LoadTools,
    #[serde(rename = "startaction")]
    StartAction,
    #[serde(rename = "continueaction")]
    ContinueAction,
    #[serde(rename = "enablelongthink")]
    EnableLongThink,
    #[serde(rename = "abort")]
    Abort,
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

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct IdeationResponse {
    pub reasoning_summary: String,
    pub needs_more_iteration: bool,
    pub context_search_request: Option<String>,
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
pub struct ContextHints {
    pub recommended_files: Vec<RecommendedFile>,
    pub search_summary: String,
    pub search_conclusion: SearchConclusion,
    pub search_terms_used: Vec<String>,
    pub knowledge_bases_searched: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extracted_output: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextChunkMatch {
    pub path: String,
    pub document_title: String,
    pub document_id: String,
    pub knowledge_base_id: String,
    pub chunk_index: i32,
    pub relevance_score: f32,
    pub excerpt: String,
    pub source: String,
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

/// Response from spawning a child thread
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpawnThreadResponse {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub thread_id: i64,
    pub status: String,
    pub message: String,
}
