use models::{
    AiKnowledgeBase, AiTool, AiToolConfiguration, ProjectTaskBoardAssignmentTarget,
    ProjectTaskBoardItemAssignmentEventDetails, ProjectTaskBoardItemMetadata, SchemaField,
    thread_event::TaskRoutingEventPayload,
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
pub struct LlmToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmToolResult {
    pub call_id: String,
    pub name: String,
    pub output: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmHistoryPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<LlmInlineData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<LlmToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<LlmToolResult>,
}

impl LlmHistoryPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            inline_data: None,
            tool_call: None,
            tool_result: None,
        }
    }

    pub fn inline_data(mime_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            text: None,
            inline_data: Some(LlmInlineData {
                mime_type: mime_type.into(),
                data: data.into(),
            }),
            tool_call: None,
            tool_result: None,
        }
    }

    pub fn tool_call(call: LlmToolCall) -> Self {
        Self {
            text: None,
            inline_data: None,
            tool_call: Some(call),
            tool_result: None,
        }
    }

    pub fn tool_result(result: LlmToolResult) -> Self {
        Self {
            text: None,
            inline_data: None,
            tool_call: None,
            tool_result: Some(result),
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
pub struct AgentLoopContext {
    pub runtime: AgentLoopRuntimeContext,
    pub conversation: AgentLoopConversationContext,
    pub thread: AgentLoopThreadContext,
    pub resources: AgentLoopResourceContext,
    pub task: AgentLoopTaskContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLoopRuntimeContext {
    pub current_datetime_utc: String,
    pub iteration_info: IterationInfo,
    /// One-turn harness signals; drained each iteration, never accumulated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_signals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLoopConversationContext {
    pub user_request: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triggering_event: Option<ThreadEventPromptItem>,
    #[serde(default)]
    pub input_safety_signals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLoopThreadContext {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub id: i64,
    pub title: String,
    pub purpose: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsibility: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLoopResourceContext {
    pub available_tools: Vec<ToolPromptItem>,
    /// Tool-name -> enabled, for *this* turn (catalog/external tools after the
    /// per-agent denylist, plus the meta tools gated by thread type). Lets the
    /// role prompts guard tool-specific guidance with
    /// `{{#if resources.enabled_tools.<name>}}` so we never instruct the agent
    /// to use a tool it doesn't have.
    #[serde(default)]
    pub enabled_tools: std::collections::BTreeMap<String, bool>,
    pub available_knowledge_bases: Vec<KnowledgeBasePromptItem>,
    #[serde(default)]
    pub available_system_skills: Vec<SkillPromptItem>,
    #[serde(default)]
    pub available_agent_skills: Vec<SkillPromptItem>,
    #[serde(default)]
    pub available_sub_agents: Vec<SubAgentPromptInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLoopTaskContext {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_journal_tail: Option<String>,
    #[serde(default)]
    pub thread_assignment_queue: Vec<ProjectTaskBoardAssignmentPromptItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_graph_view: Option<String>,
    /// Live user-feedback comments on the active board item, refreshed every
    /// iteration so resolved items flip to `[resolved]` immediately. Coordinator
    /// threads only; empty for everyone else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comment_timeline: Vec<CommentTimelinePromptItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommentTimelinePromptItem {
    pub id: String,
    pub body: String,
    pub created_at: String,
    pub resolved: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<CommentAttachmentPromptItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommentAttachmentPromptItem {
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLoopPromptEnvelope {
    #[serde(flatten)]
    pub base: AgentLoopContext,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connected_external_integrations: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_system_instructions: Option<String>,
    /// The latest user-originated event (comment, freeform clarification
    /// reply, steer, or user message). Surfaced as its own block at the
    /// top of the live context so the agent reads the freshest steer
    /// before anything else.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub most_recent_user_input: Option<MostRecentUserInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sibling_thread_tail: Option<LastSiblingThreadTail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_context_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MostRecentUserInput {
    /// `comment` | `freeform_clarification` | `steer` | `user_message`
    pub source: String,
    pub text: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastSiblingThreadTail {
    pub thread_id: String,
    pub thread_label: String,
    pub messages: Vec<SiblingTailMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiblingTailMessage {
    pub timestamp: String,
    pub kind: String,
    pub body: String,
}

fn mcp_json_schema_to_fields(schema: &serde_json::Value) -> Vec<SchemaField> {
    let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) else {
        return Vec::new();
    };
    let required_set: std::collections::HashSet<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    properties
        .iter()
        .map(|(name, prop)| {
            let field_type = match prop
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("string")
            {
                "integer" | "number" => "NUMBER",
                "boolean" => "BOOLEAN",
                "array" => "ARRAY",
                "object" => "OBJECT",
                _ => "STRING",
            };
            SchemaField {
                name: name.clone(),
                field_type: field_type.to_string(),
                required: required_set.contains(name.as_str()),
                description: prop
                    .get("description")
                    .and_then(|d| d.as_str())
                    .map(|s| s.to_string()),
                title: prop
                    .get("title")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string()),
                enum_values: prop
                    .get("enum")
                    .and_then(|e| e.as_array())
                    .map(|arr| arr.clone()),
                ..SchemaField::default()
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPromptItem {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub tool_type: String,
    #[serde(default)]
    pub approval_action: models::ApprovalAction,
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
            AiToolConfiguration::Mcp(config) => config
                .input_schema
                .as_ref()
                .map(mcp_json_schema_to_fields)
                .unwrap_or_default(),
            AiToolConfiguration::Api(config) => {
                config.request_body_schema.clone().unwrap_or_default()
            }
            AiToolConfiguration::PlatformEvent(_) => Vec::new(),
            AiToolConfiguration::Virtual(config) => config
                .input_schema
                .as_ref()
                .map(mcp_json_schema_to_fields)
                .unwrap_or_default(),
        };

        Self {
            name: crate::json::tool_calls::agent_facing_tool_name(&tool.name).to_string(),
            description: tool.description.clone(),
            tool_type: String::from(tool.tool_type.clone()),
            approval_action: tool.approval_action,
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
        default,
        with = "models::utils::serde::i64_as_string_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub board_item_id: Option<i64>,
    #[serde(
        default,
        with = "models::utils::serde::i64_as_string_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub caused_by_thread_id: Option<i64>,
    pub payload: ThreadEventPromptPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ThreadEventPromptPayload {
    TaskRouting {
        payload: TaskRoutingEventPayload,
    },
    AssignmentExecution {
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<BoardItemMountPromptInfo>,
    #[serde(
        default,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<BoardItemSchedulePromptInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardItemMountPromptInfo {
    pub mount_path: String,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardItemSchedulePromptInfo {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<String>,
    pub next_run_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<String>,
    pub overlap_policy: String,
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
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_target: Option<ProjectTaskBoardAssignmentTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationInfo {
    pub current_iteration: usize,
    pub max_iterations: usize,
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
