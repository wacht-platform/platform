use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Serialize, Deserialize, Clone)]
pub struct AgentThreadState {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub actor_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub project_id: i64,
    pub title: String,
    pub thread_visibility: String,
    pub thread_purpose: String,
    pub responsibility: Option<String>,
    pub reusable: bool,
    pub accepts_assignments: bool,
    pub capability_tags: Vec<String>,
    pub system_instructions: Option<String>,
    pub last_activity_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub execution_state: Option<ThreadExecutionState>,
    pub status: AgentThreadStatus,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub enum AgentThreadStatus {
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "waiting_for_input")]
    WaitingForInput,
    #[serde(rename = "interrupted")]
    Interrupted,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
}

// Implementation helpers
impl AgentThreadStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::WaitingForInput)
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

impl Default for AgentThreadStatus {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ThreadExecutionState {
    #[serde(default)]
    pub loaded_external_tool_ids: Vec<i64>,
    #[serde(default)]
    pub virtual_tool_cache_snapshot: Vec<crate::AiTool>,
    pub pending_approval_request: Option<ToolApprovalRequestState>,
    #[serde(default)]
    pub assignment_outcome_override: Option<ThreadAssignmentOutcomeOverride>,
    #[serde(default)]
    pub task_journal_start_hash: Option<String>,
    #[serde(default)]
    pub conversation_compaction_state: ConversationCompactionState,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PromptCacheState {
    pub cache_key: String,
    pub model_name: String,
    pub cache_name: String,
    #[serde(default)]
    pub prefix_signature: String,
    #[serde(default)]
    pub cached_contents_signature: String,
    #[serde(default)]
    pub cached_content_count: usize,
    pub expire_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ConversationCompactionState {
    #[serde(default)]
    pub last_prompt_token_count: u32,
    #[serde(default)]
    pub max_prompt_token_count_seen: u32,
    #[serde(default)]
    pub last_total_token_count: u32,
    #[serde(default)]
    pub last_compacted_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ThreadAssignmentOutcomeOverride {
    pub assignment_status: String,
    pub result_status: Option<String>,
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ToolApprovalRequestState {
    #[serde(default)]
    pub request_message_id: Option<String>,
    pub description: String,
    pub tools: Vec<RequestedToolApprovalState>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RequestedToolApprovalState {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub tool_id: i64,
    pub tool_name: String,
    pub tool_description: Option<String>,
}

// String conversions for database storage
impl Display for AgentThreadStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentThreadStatus::Idle => write!(f, "idle"),
            AgentThreadStatus::Running => write!(f, "running"),
            AgentThreadStatus::WaitingForInput => write!(f, "waiting_for_input"),
            AgentThreadStatus::Interrupted => write!(f, "interrupted"),
            AgentThreadStatus::Completed => write!(f, "completed"),
            AgentThreadStatus::Failed => write!(f, "failed"),
        }
    }
}

impl FromStr for AgentThreadStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "idle" => Ok(AgentThreadStatus::Idle),
            "running" => Ok(AgentThreadStatus::Running),
            "waiting_for_input" => Ok(AgentThreadStatus::WaitingForInput),
            "interrupted" => Ok(AgentThreadStatus::Interrupted),
            "completed" => Ok(AgentThreadStatus::Completed),
            "failed" => Ok(AgentThreadStatus::Failed),
            _ => Err(()),
        }
    }
}
