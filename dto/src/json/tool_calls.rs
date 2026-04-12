use super::agent_executor::{
    LocalKnowledgeSearchType, MemorySearchApproach, MemorySource, SearchDepth,
};
use super::memory::MemoryCategory;
use models::{FlexibleI64, InternalToolType, ProjectTaskBoardAssignmentSpec};
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn default_true() -> bool {
    true
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SearchToolsParams {
    pub queries: Vec<String>,
    #[serde(default)]
    pub max_results_per_query: Option<usize>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LoadToolsParams {
    pub tool_names: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ReadImageParams {
    pub path: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ReadFileParams {
    pub path: String,
    #[serde(default)]
    pub start_line: Option<usize>,
    #[serde(default)]
    pub end_line: Option<usize>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct WriteFileParams {
    pub path: String,
    pub content: String,
    #[serde(default)]
    pub append: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct EditFileParams {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub new_content: String,
    #[serde(default)]
    pub live_slice_hash: Option<String>,
    #[serde(default)]
    pub dangerously_skip_slice_comparison: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ExecuteCommandParams {
    pub command: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SleepParams {
    pub duration_ms: u64,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SnapshotExecutionStateParams {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct WebSearchParams {
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub search_queries: Vec<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub max_results: Option<u32>,
    #[serde(default)]
    pub include_domains: Vec<String>,
    #[serde(default)]
    pub exclude_domains: Vec<String>,
    #[serde(default)]
    pub after_date: Option<String>,
    #[serde(default)]
    pub excerpt_max_chars_per_result: Option<u32>,
    #[serde(default)]
    pub excerpt_max_chars_total: Option<u32>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct UrlContentParams {
    pub urls: Vec<String>,
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub search_queries: Vec<String>,
    #[serde(default = "default_true")]
    pub excerpts: bool,
    #[serde(default)]
    pub full_content: bool,
    #[serde(default)]
    pub excerpt_max_chars_per_result: Option<u32>,
    #[serde(default)]
    pub excerpt_max_chars_total: Option<u32>,
    #[serde(default)]
    pub full_content_max_chars_per_result: Option<u32>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SearchKnowledgebaseParams {
    pub query: String,
    #[serde(default)]
    pub search_type: Option<LocalKnowledgeSearchType>,
    #[serde(default)]
    pub knowledge_base_ids: Option<Vec<String>>,
    #[serde(default)]
    pub max_results: Option<u32>,
    #[serde(default)]
    pub include_associated_chunks: Option<bool>,
    #[serde(default)]
    pub max_associated_chunks_per_document: Option<u32>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SaveMemoryParams {
    pub content: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LoadMemoryParams {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub categories: Vec<MemoryCategory>,
    #[serde(default)]
    pub sources: Vec<MemorySource>,
    #[serde(default)]
    pub depth: Option<SearchDepth>,
    #[serde(default)]
    pub search_approach: MemorySearchApproach,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ProjectTaskScheduleParams {
    pub kind: String,
    pub next_run_at: String,
    #[serde(default)]
    pub interval_seconds: Option<i64>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CreateProjectTaskParams {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub parent_task_key: Option<String>,
    #[serde(default)]
    pub schedule: Option<ProjectTaskScheduleParams>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct UpdateProjectTaskParams {
    pub task_key: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub schedule: Option<ProjectTaskScheduleParams>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AssignProjectTaskParams {
    pub task_key: String,
    pub assignments: Vec<ProjectTaskBoardAssignmentSpec>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ListThreadsParams {
    #[serde(default)]
    pub include_conversation_threads: bool,
    #[serde(default)]
    pub include_archived: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CreateThreadParams {
    pub title: String,
    #[serde(default)]
    pub assigned_agent_name: Option<String>,
    #[serde(default)]
    pub responsibility: Option<String>,
    #[serde(default)]
    pub system_instructions: Option<String>,
    #[serde(default)]
    pub reusable: Option<bool>,
    #[serde(default)]
    pub accepts_assignments: Option<bool>,
    #[serde(default)]
    pub capability_tags: Option<Vec<String>>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct UpdateThreadParams {
    pub thread_id: FlexibleI64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub responsibility: Option<String>,
    #[serde(default)]
    pub system_instructions: Option<String>,
    #[serde(default)]
    pub reusable: Option<bool>,
    #[serde(default)]
    pub accepts_assignments: Option<bool>,
    #[serde(default)]
    pub capability_tags: Option<Vec<String>>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TaskGraphAddNodeParams {
    #[serde(default)]
    pub node_ref: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub max_retries: Option<i32>,
    #[serde(default)]
    pub input: Option<Value>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TaskGraphAddDependencyParams {
    #[serde(default)]
    pub from_node_id: Option<FlexibleI64>,
    #[serde(default)]
    pub from_node_ref: Option<String>,
    #[serde(default)]
    pub to_node_id: Option<FlexibleI64>,
    #[serde(default)]
    pub to_node_ref: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TaskGraphNodeTargetParams {
    #[serde(default)]
    pub node_id: Option<FlexibleI64>,
    #[serde(default)]
    pub node_ref: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TaskGraphCompleteNodeParams {
    #[serde(flatten)]
    pub target: TaskGraphNodeTargetParams,
    #[serde(default)]
    pub output: Option<Value>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TaskGraphFailNodeParams {
    #[serde(flatten)]
    pub target: TaskGraphNodeTargetParams,
    #[serde(default)]
    pub error: Option<Value>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TaskGraphMarkCompletedParams {
    pub handoff_path: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TaskGraphMarkFailedParams {
    pub handoff_path: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ExternalToolCall {
    pub tool_name: String,
    #[serde(default)]
    pub input: Value,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ToolExecutionPlan {
    pub tool_calls: Vec<ToolCallRequest>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum ToolCallRequest {
    SearchTools {
        params: SearchToolsParams,
    },
    LoadTools {
        params: LoadToolsParams,
    },
    ReadImage {
        params: ReadImageParams,
    },
    ReadFile {
        params: ReadFileParams,
    },
    WriteFile {
        params: WriteFileParams,
    },
    EditFile {
        params: EditFileParams,
    },
    ExecuteCommand {
        params: ExecuteCommandParams,
    },
    Sleep {
        params: SleepParams,
    },
    SnapshotExecutionState {
        params: SnapshotExecutionStateParams,
    },
    WebSearch {
        params: WebSearchParams,
    },
    UrlContent {
        params: UrlContentParams,
    },
    SearchKnowledgebase {
        params: SearchKnowledgebaseParams,
    },
    LoadMemory {
        params: LoadMemoryParams,
    },
    SaveMemory {
        params: SaveMemoryParams,
    },
    CreateProjectTask {
        params: CreateProjectTaskParams,
    },
    UpdateProjectTask {
        params: UpdateProjectTaskParams,
    },
    AssignProjectTask {
        params: AssignProjectTaskParams,
    },
    ListThreads {
        params: ListThreadsParams,
    },
    CreateThread {
        params: CreateThreadParams,
    },
    UpdateThread {
        params: UpdateThreadParams,
    },
    TaskGraphAddNode {
        params: TaskGraphAddNodeParams,
    },
    TaskGraphAddDependency {
        params: TaskGraphAddDependencyParams,
    },
    TaskGraphMarkInProgress {
        params: TaskGraphNodeTargetParams,
    },
    TaskGraphCompleteNode {
        params: TaskGraphCompleteNodeParams,
    },
    TaskGraphFailNode {
        params: TaskGraphFailNodeParams,
    },
    TaskGraphMarkCompleted {
        params: TaskGraphMarkCompletedParams,
    },
    TaskGraphMarkFailed {
        params: TaskGraphMarkFailedParams,
    },
    External(ExternalToolCall),
}

impl ToolCallRequest {
    pub fn tool_name(&self) -> &str {
        match self {
            Self::SearchTools { .. } => "search_tools",
            Self::LoadTools { .. } => "load_tools",
            Self::ReadImage { .. } => "read_image",
            Self::ReadFile { .. } => "read_file",
            Self::WriteFile { .. } => "write_file",
            Self::EditFile { .. } => "edit_file",
            Self::ExecuteCommand { .. } => "execute_command",
            Self::Sleep { .. } => "sleep",
            Self::SnapshotExecutionState { .. } => "snapshot_execution_state",
            Self::WebSearch { .. } => "web_search",
            Self::UrlContent { .. } => "url_content",
            Self::SearchKnowledgebase { .. } => "search_knowledgebase",
            Self::LoadMemory { .. } => "load_memory",
            Self::SaveMemory { .. } => "save_memory",
            Self::CreateProjectTask { .. } => "create_project_task",
            Self::UpdateProjectTask { .. } => "update_project_task",
            Self::AssignProjectTask { .. } => "assign_project_task",
            Self::ListThreads { .. } => "list_threads",
            Self::CreateThread { .. } => "create_thread",
            Self::UpdateThread { .. } => "update_thread",
            Self::TaskGraphAddNode { .. } => "task_graph_add_node",
            Self::TaskGraphAddDependency { .. } => "task_graph_add_dependency",
            Self::TaskGraphMarkInProgress { .. } => "task_graph_mark_in_progress",
            Self::TaskGraphCompleteNode { .. } => "task_graph_complete_node",
            Self::TaskGraphFailNode { .. } => "task_graph_fail_node",
            Self::TaskGraphMarkCompleted { .. } => "task_graph_mark_completed",
            Self::TaskGraphMarkFailed { .. } => "task_graph_mark_failed",
            Self::External(call) => call.tool_name.as_str(),
        }
    }

    pub fn internal_tool_type(&self) -> Option<InternalToolType> {
        match self {
            Self::SearchTools { .. } => Some(InternalToolType::SearchTools),
            Self::LoadTools { .. } => Some(InternalToolType::LoadTools),
            Self::ReadImage { .. } => Some(InternalToolType::ReadImage),
            Self::ReadFile { .. } => Some(InternalToolType::ReadFile),
            Self::WriteFile { .. } => Some(InternalToolType::WriteFile),
            Self::EditFile { .. } => Some(InternalToolType::EditFile),
            Self::ExecuteCommand { .. } => Some(InternalToolType::ExecuteCommand),
            Self::Sleep { .. } => Some(InternalToolType::Sleep),
            Self::SnapshotExecutionState { .. } => Some(InternalToolType::SnapshotExecutionState),
            Self::WebSearch { .. } => Some(InternalToolType::WebSearch),
            Self::UrlContent { .. } => Some(InternalToolType::UrlContent),
            Self::SearchKnowledgebase { .. } => Some(InternalToolType::SearchKnowledgebase),
            Self::LoadMemory { .. } => Some(InternalToolType::LoadMemory),
            Self::SaveMemory { .. } => Some(InternalToolType::SaveMemory),
            Self::CreateProjectTask { .. } => Some(InternalToolType::CreateProjectTask),
            Self::UpdateProjectTask { .. } => Some(InternalToolType::UpdateProjectTask),
            Self::AssignProjectTask { .. } => Some(InternalToolType::AssignProjectTask),
            Self::ListThreads { .. } => Some(InternalToolType::ListThreads),
            Self::CreateThread { .. } => Some(InternalToolType::CreateThread),
            Self::UpdateThread { .. } => Some(InternalToolType::UpdateThread),
            Self::TaskGraphAddNode { .. } => Some(InternalToolType::TaskGraphAddNode),
            Self::TaskGraphAddDependency { .. } => Some(InternalToolType::TaskGraphAddDependency),
            Self::TaskGraphMarkInProgress { .. } => Some(InternalToolType::TaskGraphMarkInProgress),
            Self::TaskGraphCompleteNode { .. } => Some(InternalToolType::TaskGraphCompleteNode),
            Self::TaskGraphFailNode { .. } => Some(InternalToolType::TaskGraphFailNode),
            Self::TaskGraphMarkCompleted { .. } => Some(InternalToolType::TaskGraphMarkCompleted),
            Self::TaskGraphMarkFailed { .. } => Some(InternalToolType::TaskGraphMarkFailed),
            Self::External(_) => None,
        }
    }

    pub fn input_value(&self) -> Result<Value, serde_json::Error> {
        match self {
            Self::SearchTools { params, .. } => serde_json::to_value(params),
            Self::LoadTools { params, .. } => serde_json::to_value(params),
            Self::ReadImage { params, .. } => serde_json::to_value(params),
            Self::ReadFile { params, .. } => serde_json::to_value(params),
            Self::WriteFile { params, .. } => serde_json::to_value(params),
            Self::EditFile { params, .. } => serde_json::to_value(params),
            Self::ExecuteCommand { params, .. } => serde_json::to_value(params),
            Self::Sleep { params, .. } => serde_json::to_value(params),
            Self::SnapshotExecutionState { params, .. } => serde_json::to_value(params),
            Self::WebSearch { params, .. } => serde_json::to_value(params),
            Self::UrlContent { params, .. } => serde_json::to_value(params),
            Self::SearchKnowledgebase { params, .. } => serde_json::to_value(params),
            Self::LoadMemory { params, .. } => serde_json::to_value(params),
            Self::SaveMemory { params, .. } => serde_json::to_value(params),
            Self::CreateProjectTask { params, .. } => serde_json::to_value(params),
            Self::UpdateProjectTask { params, .. } => serde_json::to_value(params),
            Self::AssignProjectTask { params, .. } => serde_json::to_value(params),
            Self::ListThreads { params, .. } => serde_json::to_value(params),
            Self::CreateThread { params, .. } => serde_json::to_value(params),
            Self::UpdateThread { params, .. } => serde_json::to_value(params),
            Self::TaskGraphAddNode { params, .. } => serde_json::to_value(params),
            Self::TaskGraphAddDependency { params, .. } => serde_json::to_value(params),
            Self::TaskGraphMarkInProgress { params, .. } => serde_json::to_value(params),
            Self::TaskGraphCompleteNode { params, .. } => serde_json::to_value(params),
            Self::TaskGraphFailNode { params, .. } => serde_json::to_value(params),
            Self::TaskGraphMarkCompleted { params, .. } => serde_json::to_value(params),
            Self::TaskGraphMarkFailed { params, .. } => serde_json::to_value(params),
            Self::External(call) => Ok(call.input.clone()),
        }
    }
}
