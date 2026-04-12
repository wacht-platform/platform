use models::{
    AiKnowledgeBase, AiTool, AiToolConfiguration, ProjectTaskBoardAssignmentTarget,
    ProjectTaskBoardItemAssignmentEventDetails, ProjectTaskBoardItemMetadata, SchemaField,
    thread_event::{
        ApprovalResponseReceivedEventPayload, TaskRoutingEventPayload,
        ThreadConversationEventPayload,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmInlineData {
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmHistoryPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<LlmInlineData>,
}

impl LlmHistoryPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            inline_data: None,
        }
    }

    pub fn inline_data(mime_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            text: None,
            inline_data: Some(LlmInlineData {
                mime_type: mime_type.into(),
                data: data.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmHistoryEntry {
    pub role: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<LlmHistoryPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(rename = "type")]
    pub entry_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl LlmHistoryEntry {
    pub fn with_parts(
        role: impl Into<String>,
        entry_type: impl Into<String>,
        timestamp: Option<String>,
        parts: Vec<LlmHistoryPart>,
    ) -> Self {
        Self {
            role: role.into(),
            parts,
            content: None,
            timestamp,
            entry_type: entry_type.into(),
            sender: None,
            metadata: None,
        }
    }

    pub fn with_content(
        role: impl Into<String>,
        entry_type: impl Into<String>,
        timestamp: Option<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: role.into(),
            parts: Vec::new(),
            content: Some(content.into()),
            timestamp,
            entry_type: entry_type.into(),
            sender: None,
            metadata: None,
        }
    }
}

// Template Context for LLM Calls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextStepDecisionContext {
    pub runtime: NextStepDecisionRuntimeContext,
    pub conversation: NextStepDecisionConversationContext,
    pub thread: NextStepDecisionThreadContext,
    pub resources: NextStepDecisionResourceContext,
    pub task: NextStepDecisionTaskContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextStepDecisionRuntimeContext {
    pub current_datetime_utc: String,
    pub iteration_info: IterationInfo,
    #[serde(default)]
    pub long_think_mode_active: bool,
    #[serde(default)]
    pub long_think_input_tokens_available: u32,
    #[serde(default)]
    pub long_think_output_tokens_available: u32,
    #[serde(default = "default_long_think_input_token_budget")]
    pub long_think_input_token_budget: u32,
    #[serde(default = "default_long_think_output_token_budget")]
    pub long_think_output_token_budget: u32,
    #[serde(default = "default_long_think_window_minutes")]
    pub long_think_window_minutes: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_think_nudge: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steer_visibility_nudge: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextStepDecisionConversationContext {
    pub user_request: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triggering_event: Option<ThreadEventPromptItem>,
    #[serde(default)]
    pub input_safety_signals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextStepDecisionThreadContext {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub id: i64,
    pub title: String,
    pub purpose: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsibility: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextStepDecisionResourceContext {
    pub available_tools: Vec<ToolPromptItem>,
    pub available_knowledge_bases: Vec<KnowledgeBasePromptItem>,
    #[serde(default)]
    pub available_system_skills: Vec<SkillPromptItem>,
    #[serde(default)]
    pub available_agent_skills: Vec<SkillPromptItem>,
    #[serde(default)]
    pub available_sub_agents: Vec<SubAgentPromptInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextStepDecisionTaskContext {
    #[serde(default)]
    pub project_task_board_items: Vec<ProjectTaskBoardPromptItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_board_item: Option<ProjectTaskBoardPromptItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_assignment: Option<ProjectTaskBoardAssignmentPromptItem>,
    #[serde(default)]
    pub active_board_item_assignments: Vec<ProjectTaskBoardAssignmentPromptItem>,
    #[serde(default)]
    pub recent_assignment_history: Vec<ProjectTaskBoardAssignmentPromptItem>,
    #[serde(default)]
    pub active_board_item_events: Vec<ProjectTaskBoardItemEventPromptItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_journal_tail: Option<String>,
    #[serde(default)]
    pub thread_assignment_queue: Vec<ProjectTaskBoardAssignmentPromptItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_graph_view: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextStepDecisionPromptEnvelope {
    #[serde(flatten)]
    pub base: NextStepDecisionContext,
    pub agent_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_description: Option<String>,
    #[serde(default)]
    pub conversation_history_prefix: Vec<LlmHistoryEntry>,
    pub current_request_entry: LlmHistoryEntry,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub discoverable_external_tool_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loaded_external_tool_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_system_instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_context_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPromptItem {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub tool_type: String,
    #[serde(default)]
    pub requires_user_approval: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_schema: Vec<SchemaField>,
}

impl ToolPromptItem {
    pub fn from_tool(tool: &AiTool) -> Self {
        let input_schema = match &tool.configuration {
            AiToolConfiguration::Internal(config) => {
                config.input_schema.clone().unwrap_or_default()
            }
            AiToolConfiguration::CodeRunner(config) => {
                config.input_schema.clone().unwrap_or_default()
            }
            AiToolConfiguration::UseExternalService(config) => {
                config.input_schema.clone().unwrap_or_default()
            }
            AiToolConfiguration::Api(config) => {
                config.request_body_schema.clone().unwrap_or_default()
            }
            AiToolConfiguration::PlatformEvent(_) => Vec::new(),
        };

        Self {
            name: tool.name.clone(),
            description: tool.description.clone(),
            tool_type: String::from(tool.tool_type.clone()),
            requires_user_approval: tool.requires_user_approval,
            input_schema,
        }
    }

    pub fn summary_line(&self) -> String {
        let name = self.name.as_str();
        let description = self.description.as_deref().unwrap_or("No description");
        let input_fields = self
            .input_schema
            .iter()
            .filter_map(|field| {
                if field.name.trim().is_empty() {
                    return None;
                }

                let field_type = if field.field_type.trim().is_empty() {
                    "any"
                } else {
                    field.field_type.trim()
                }
                .to_lowercase();
                let mut rendered = if field.required {
                    format!("{}*<{}>", field.name.trim(), field_type)
                } else {
                    format!("{}<{}>", field.name.trim(), field_type)
                };

                if let Some(description) = field.description.as_deref() {
                    let shortened = shorten_summary_description(description);
                    if !shortened.is_empty() {
                        rendered.push_str(" - ");
                        rendered.push_str(&shortened);
                    }
                }

                Some(rendered)
            })
            .collect::<Vec<_>>();

        if input_fields.is_empty() {
            format!("- {name}: {description}")
        } else {
            format!(
                "- {name}: {description} Inputs: {}",
                input_fields.join(", ")
            )
        }
    }

    pub fn summarize_list(tools: &[Self]) -> String {
        tools
            .iter()
            .map(Self::summary_line)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeBasePromptItem {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

impl KnowledgeBasePromptItem {
    pub fn from_knowledge_base(knowledge_base: &AiKnowledgeBase) -> Self {
        Self {
            id: knowledge_base.id,
            name: knowledge_base.name.clone(),
            description: knowledge_base.description.clone(),
        }
    }

    pub fn summary_line(&self) -> String {
        let description = self.description.as_deref().unwrap_or("No description");
        format!("- {}: {}", self.name, description)
    }

    pub fn summarize_list(knowledge_bases: &[Self]) -> String {
        knowledge_bases
            .iter()
            .map(Self::summary_line)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillPromptItem {
    pub slug: String,
    pub mount_path: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub source: String,
}

impl SkillPromptItem {
    pub fn summary_line(&self) -> String {
        let description = self.description.as_deref().unwrap_or("No description");
        format!("- {} at `{}`: {}", self.slug, self.mount_path, description)
    }

    pub fn summarize_list(skills: &[Self]) -> String {
        skills
            .iter()
            .map(Self::summary_line)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadEventPromptItem {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub event_id: i64,
    pub event_type: String,
    #[serde(
        with = "models::utils::serde::i64_as_string_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub board_item_id: Option<i64>,
    #[serde(
        with = "models::utils::serde::i64_as_string_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub caused_by_thread_id: Option<i64>,
    #[serde(
        with = "models::utils::serde::i64_as_string_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub caused_by_conversation_id: Option<i64>,
    pub payload: ThreadEventPromptPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ThreadEventPromptPayload {
    Conversation {
        payload: ThreadConversationEventPayload,
    },
    ApprovalResponseReceived {
        payload: ApprovalResponseReceivedEventPayload,
    },
    TaskRouting {
        payload: TaskRoutingEventPayload,
    },
    AssignmentExecution {
        payload: ProjectTaskBoardItemAssignmentEventDetails,
    },
    AssignmentOutcomeReview {
        payload: ProjectTaskBoardItemAssignmentEventDetails,
    },
    Raw {
        raw_json: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentPromptInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTaskBoardPromptItem {
    pub task_key: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    #[serde(
        with = "models::utils::serde::i64_as_string_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub assigned_thread_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_task_key: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub child_task_keys: Vec<String>,
    pub metadata: ProjectTaskBoardItemMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTaskBoardAssignmentPromptItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub assignment_id: i64,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub board_item_id: i64,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub thread_id: i64,
    pub assignment_role: String,
    pub assignment_order: i32,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handoff_file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_target: Option<ProjectTaskBoardAssignmentTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTaskBoardItemEventPromptItem {
    pub event_type: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_markdown: Option<String>,
    #[serde(
        with = "models::utils::serde::i64_as_string_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub thread_id: Option<i64>,
    #[serde(
        with = "models::utils::serde::i64_as_string_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub execution_run_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_details_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment_details: Option<ProjectTaskBoardItemAssignmentEventDetails>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationInfo {
    pub current_iteration: usize,
    pub max_iterations: usize,
}

fn default_long_think_input_token_budget() -> u32 {
    2_000_000
}

fn default_long_think_output_token_budget() -> u32 {
    300_000
}

fn default_long_think_window_minutes() -> u32 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationContext {
    pub conversation_history: Vec<LlmHistoryEntry>,
    pub user_request: String,
    pub task_results: HashMap<String, Value>,
    pub available_tools: Vec<ToolPromptItem>,
    pub available_knowledge_bases: Vec<KnowledgeBasePromptItem>,
}

fn shorten_summary_description(description: &str) -> String {
    let mut shortened = description.trim().to_string();
    if let Some((first, _)) = shortened.split_once('.') {
        shortened = first.trim().to_string();
    }
    if shortened.chars().count() > 72 {
        shortened = shortened.chars().take(72).collect::<String>();
        shortened.push_str("...");
    }
    shortened
}

// LLM Generation Config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMGenerationConfig {
    pub contents: Vec<LLMContent>,
    #[serde(rename = "generationConfig")]
    pub generation_config: GenerationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMContent {
    pub parts: Vec<LLMPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMPart {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationConfig {
    pub temperature: f32,
    #[serde(rename = "topK")]
    pub top_k: i32,
    #[serde(rename = "topP")]
    pub top_p: f32,
    #[serde(rename = "maxOutputTokens")]
    pub max_output_tokens: i32,
}
