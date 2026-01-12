use crate::context::ContextOrchestrator;
use crate::filesystem::{shell::ShellExecutor, AgentFilesystem};
use crate::tools::ToolExecutor;

use common::error::AppError;
use common::state::AppState;
use dto::json::agent_executor::{ConversationInsights, ObjectiveDefinition, TaskExecutionResult};
use dto::json::StreamEvent;
use models::{
    AgentExecutionState, AiAgentWithFeatures, ConversationRecord, ExecutionContextStatus,
    MemoryRecord, WorkflowExecutionState,
};
use models::{
    AiTool, AiToolConfiguration, AiToolType, InternalToolConfiguration, InternalToolType,
    SchemaField, UseExternalServiceToolConfiguration, UseExternalServiceToolType,
};
use queries::{GetExecutionContextQuery, Query};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum ResumeContext {
    PlatformFunction(String, Value),
    UserInput(String),
}

pub struct AgentExecutor {
    pub(super) agent: AiAgentWithFeatures,
    pub(super) app_state: AppState,
    pub(super) context_id: i64,
    pub(super) conversations: Vec<ConversationRecord>,
    pub(super) context_orchestrator: ContextOrchestrator,
    pub(super) tool_executor: ToolExecutor,
    pub(super) channel: tokio::sync::mpsc::Sender<StreamEvent>,
    pub(super) memories: Vec<MemoryRecord>,
    pub(super) loaded_memory_ids: std::collections::HashSet<i64>,
    pub(super) user_request: String,
    pub(super) current_objective: Option<ObjectiveDefinition>,
    pub(super) conversation_insights: Option<ConversationInsights>,
    pub(super) task_results: HashMap<String, TaskExecutionResult>,
    pub(super) current_workflow_id: Option<i64>,
    pub(super) current_workflow_state: Option<HashMap<String, Value>>,
    pub(super) current_workflow_node_id: Option<String>,
    pub(super) current_workflow_execution_path: Vec<String>,
    pub(super) system_instructions: Option<String>,
    pub(super) filesystem: AgentFilesystem,
    pub(super) shell: ShellExecutor,
    pub(super) teams_enabled: bool,
    pub(super) clickup_enabled: bool,
    pub(super) context_title: String,
}

pub struct AgentExecutorBuilder {
    agent: AiAgentWithFeatures,
    app_state: AppState,
    context_id: i64,
    channel: tokio::sync::mpsc::Sender<StreamEvent>,
    context_title: String,
}

impl AgentExecutorBuilder {
    pub fn new(
        agent: AiAgentWithFeatures,
        context_id: i64,
        app_state: AppState,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
        context_title: String,
    ) -> Self {
        Self {
            agent,
            context_id,
            app_state,
            channel,
            context_title,
        }
    }

    pub async fn build(self) -> Result<AgentExecutor, AppError> {
        let tool_executor =
            ToolExecutor::new(self.app_state.clone(), self.agent.clone(), self.context_id)
                .with_channel(self.channel.clone());
        let context_orchestrator =
            ContextOrchestrator::new(self.app_state.clone(), self.agent.clone(), self.context_id);

        let execution_id = self.app_state.sf.next_id()?.to_string();

        let filesystem = AgentFilesystem::new(
            &self.agent.deployment_id.to_string(),
            &self.agent.id.to_string(),
            &self.context_id.to_string(),
            &execution_id,
        );

        if let Err(e) = filesystem.initialize().await {
            tracing::warn!("Failed to initialize agent filesystem: {}", e);
        }

        let shell = ShellExecutor::new(filesystem.execution_root());

        for kb in &self.agent.knowledge_bases {
            if let Err(e) = filesystem
                .link_knowledge_base(&kb.id.to_string(), &kb.name)
                .await
            {
                tracing::warn!(
                    "Failed to link knowledge base {} ({}): {}",
                    kb.name,
                    kb.id,
                    e
                );
            }
        }

        let internal_tools = vec![
            (
                "read_file",
                "Read file content. Supports line ranges. Returns total_lines for navigation.",
                InternalToolType::ReadFile,
                vec![
                    SchemaField {
                        name: "path".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Path to the file".to_string()),
                        required: true, ..Default::default()
                    },
                    SchemaField {
                        name: "start_line".to_string(),
                        field_type: "INTEGER".to_string(),
                        description: Some("Start line (1-indexed, optional)".to_string()),
                        required: false, ..Default::default()
                    },
                    SchemaField {
                        name: "end_line".to_string(),
                        field_type: "INTEGER".to_string(),
                        description: Some("End line (inclusive, optional)".to_string()),
                        required: false, ..Default::default()
                    }
                ]
            ),
            (
                "write_file",
                "Write to file. For partial writes (with start_line/end_line), must read_file first.",
                InternalToolType::WriteFile,
                vec![
                    SchemaField {
                        name: "path".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Path to write (memory/, workspace/, scratch/ only)".to_string()),
                        required: true, ..Default::default()
                    },
                    SchemaField {
                        name: "content".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Content to write".to_string()),
                        required: true, ..Default::default()
                    },
                    SchemaField {
                        name: "start_line".to_string(),
                        field_type: "INTEGER".to_string(),
                        description: Some("Replace from this line (1-indexed). Requires prior read_file.".to_string()),
                        required: false, ..Default::default()
                    },
                    SchemaField {
                        name: "end_line".to_string(),
                        field_type: "INTEGER".to_string(),
                        description: Some("Replace up to this line (inclusive). Requires prior read_file.".to_string()),
                        required: false, ..Default::default()
                    }
                ]
            ),
            (
                "list_directory",
                "List files and directories at a path.",
                InternalToolType::ListDirectory,
                vec![
                    SchemaField {
                        name: "path".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Directory path (default: '/')".to_string()),
                        required: false, ..Default::default()
                    }
                ]
            ),
            (
                "search_files",
                "Search for text patterns in files.",
                InternalToolType::SearchFiles,
                vec![
                    SchemaField {
                        name: "query".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Text or regex to search for".to_string()),
                        required: true, ..Default::default()
                    },
                    SchemaField {
                        name: "path".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Directory to search (default: '/')".to_string()),
                        required: false, ..Default::default()
                    }
                ]
            ),
            (
                "execute_command",
                "Execute a shell command. Allowed commands: cat, head, tail, grep, rg, find, ls, tree, wc, du, df, touch, mkdir, echo, cp, mv, rm, chmod, sed, awk, sort, uniq, jq, cut, tr, diff, date, whoami, pwd, printf.",
                InternalToolType::ExecuteCommand,
                vec![
                    SchemaField {
                        name: "command".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Shell command to run".to_string()),
                        required: true, ..Default::default()
                    }
                ]
            ),
            (
                "save_memory",
                "Save important information to long-term memory. Use for facts, preferences, procedures that should be remembered across sessions.",
                InternalToolType::SaveMemory,
                vec![
                    SchemaField {
                        name: "content".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("The information to remember".to_string()),
                        required: true, ..Default::default()
                    },
                    SchemaField {
                        name: "category".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Category: procedural (how-to), semantic (facts), episodic (events), working (temp)".to_string()),
                        required: true, ..Default::default()
                    },
                    SchemaField {
                        name: "importance".to_string(),
                        field_type: "NUMBER".to_string(),
                        description: Some("Importance 0.0-1.0 (default: 0.5)".to_string()),
                        required: false, ..Default::default()
                    }
                ]
            ),
            (
                "execute_python",
                "Execute a Python script from a file securely (sandboxed on Linux). Script must be in workspace.",
                InternalToolType::ExecutePython,
                vec![
                    SchemaField {
                        name: "script_path".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Relative path to script (e.g. workspace/analysis.py)".to_string()),
                        required: true, ..Default::default()
                    },
                    SchemaField {
                        name: "args".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Space-separated arguments".to_string()),
                        required: false, ..Default::default()
                    }
                ]
            ),
        ];

        let context = GetExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
            .execute(&self.app_state)
            .await?;

        let mut teams_enabled = false;
        let mut clickup_enabled = false;

        if let Some(context_group) = &context.context_group {
            let active_integrations = queries::GetActiveIntegrationsForContextQuery::new(
                self.agent.deployment_id,
                self.agent.id,
                context_group.clone(),
            )
            .execute(&self.app_state)
            .await?;

            let has_teams = active_integrations
                .iter()
                .any(|i| matches!(i.integration_type, models::IntegrationType::Teams));

            if has_teams {
                teams_enabled = true;
                tracing::info!(
                    "Context group {} has active Teams integration. Injecting Teams tools.",
                    context_group
                );

                if let Err(e) = filesystem.link_teams_activity(context_group).await {
                    tracing::warn!("Failed to link teams-activity directory: {}", e);
                }
            }

            let has_clickup = active_integrations
                .iter()
                .any(|i| matches!(i.integration_type, models::IntegrationType::ClickUp));

            if has_clickup {
                clickup_enabled = true;
                tracing::info!(
                    "Context group {} has active ClickUp integration. Injecting ClickUp tools.",
                    context_group
                );
            }
        }

        let mut current_tools = self.agent.tools.clone();
        for (name, desc, tool_type, schema) in internal_tools {
            if !current_tools.iter().any(|t| t.name == name) {
                current_tools.push(AiTool {
                    id: -1,
                    name: name.to_string(),
                    description: Some(desc.to_string()),
                    tool_type: AiToolType::Internal,
                    deployment_id: self.agent.deployment_id,
                    configuration: AiToolConfiguration::Internal(InternalToolConfiguration {
                        tool_type,
                        input_schema: Some(schema),
                    }),
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                });
            }
        }

        if teams_enabled {
            let teams_tools: Vec<(&str, &str, UseExternalServiceToolType, Vec<SchemaField>)> = vec![
                (
                    "teams_list_users",
                    "List users in the Microsoft Teams tenant. Returns up to 25 users by default.",
                    UseExternalServiceToolType::TeamsListUsers,
                    vec![
                        SchemaField {
                            name: "limit".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Max number of users to return (default: 25).".to_string()),
                            required: false, ..Default::default()
                        }
                    ]
                ),
                (
                    "teams_search_users",
                    "Search for users in the Microsoft Teams tenant by name or email.",
                    UseExternalServiceToolType::TeamsSearchUsers,
                    vec![
                        SchemaField {
                            name: "query".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Search query (name or email).".to_string()),
                            required: true, ..Default::default()
                        }
                    ]
                ),
                (
                    "teams_send_dm",
                    "Send a direct message to a user in Microsoft Teams. ALWAYS include sender_info to explain who is asking and from where. Use notify_on_reply to get notified when user responds.",
                    UseExternalServiceToolType::TeamsSendDm,
                    vec![
                        SchemaField {
                            name: "user_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The user's ID (aadObjectId) to send the message to. Get this from teams_list_users or teams_search_users.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "message".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The message content to send. MUST include context about who is asking if relaying on behalf of someone.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "sender_info".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("REQUIRED: Who is sending this and from where. Example: 'Saurav Singh from #General channel' or 'John Smith from group DM'. This is prepended to your message for clarity.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "notify_on_reply".to_string(),
                            field_type: "BOOLEAN".to_string(),
                            description: Some("If true, you will be notified in your current context when the user replies. Default: false.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "description".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Context for why you're sending this DM - helps when processing the reply. Required if notify_on_reply is true.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "context_notes".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Transient memory passed to the DM context. Include: WHO is asking (name), WHAT they need, and any context. The agent handling this DM sees these notes and can act accordingly. Example: 'John Smith asked for the project deadline - let him know when you get a response'.".to_string()),
                            required: false, ..Default::default()
                        },
                    ]
                ),
                (
                    "teams_send_context_message",
                    "Send a message to any Teams channel or chat by context ID. Use teams_list_contexts() to discover available contexts. The message will be visible to everyone in that conversation. Use notify_on_reply if you want to be notified when someone responds.",
                    UseExternalServiceToolType::TeamsSendContextMessage,
                    vec![
                        SchemaField {
                            name: "context_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The target context ID. Use teams_list_contexts() to get available context IDs.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "message".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The message to send to the channel/chat.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "sender_info".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Context about who/where this message is coming from. Example: 'From John in #Support channel'.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "notify_on_reply".to_string(),
                            field_type: "BOOLEAN".to_string(),
                            description: Some("If true, you will be notified in your current context when someone replies in that channel. Default: false.".to_string()),
                            required: false, ..Default::default()
                        },
                    ]
                ),
                (
                    "teams_list_messages",
                    "Get recent messages from a Teams conversation. Works for Team channels, group DMs, and 1:1 chats. Useful for finding meeting notifications, previous discussions, or context. Use context_id to read from another channel/chat.",
                    UseExternalServiceToolType::TeamsListMessages,
                    vec![
                        SchemaField {
                            name: "context_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Optional: Target context ID to read messages from. Use teams_list_contexts() to discover available contexts. Defaults to current context.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "count".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Batch size for fetching. If `from_date` is used, this acts as page size (system auto-paginates up to 500 messages). If no date, this is the limit (max 50).".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "from_date".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("ISO date/datetime to filter messages FROM (inclusive). Example: '2026-01-01' or '2026-01-01T09:00:00Z'.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "to_date".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("ISO date/datetime to filter messages TO (inclusive). Example: '2026-01-10' or '2026-01-10T18:00:00Z'.".to_string()),
                            required: false, ..Default::default()
                        },
                    ]
                ),
                (
                    "teams_get_meeting_recording",
                    "Get meeting recording info. PREFERRED: Pass 'join_url' from a meeting recap link to auto-extract recording. ALTERNATIVE: For channel meetings team_id is auto-detected. For DM/group chats use 'organizer_id' from callEnded events.",
                    UseExternalServiceToolType::TeamsGetMeetingRecording,
                    vec![
                        SchemaField {
                            name: "context_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Optional: Target context ID. Use teams_list_contexts() to discover available contexts. Defaults to current context.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "organizer_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("For DM/group chat recordings. The AAD Object ID of the meeting organizer from callEnded events. Not needed for channel meetings (auto-detected).".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "meeting_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The meeting ID. Optional if using search_recent.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "join_url".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The meeting join URL. Optional if using search_recent.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "search_recent".to_string(),
                            field_type: "BOOLEAN".to_string(),
                            description: Some("Set to true to search for recent recordings without needing meeting ID. Useful for ad-hoc calls.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "max_results".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Maximum number of recordings to return. Default 5.".to_string()),
                            required: false, ..Default::default()
                        },
                    ]
                ),
                (
                    "teams_analyze_meeting",
                    "Analyze a meeting recording. PREFERRED: Pass 'join_url' from a meeting recap link to auto-download and transcribe. ALTERNATIVE: Pass 'recording_id' + 'organizer_id' from teams_get_meeting_recording results. Extracts: 1) Audio transcription with speaker labels, 2) Visual content from screen shares.",
                    UseExternalServiceToolType::TeamsTranscribeMeeting,
                    vec![
                        SchemaField {
                            name: "join_url".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("PREFERRED: The meeting recap URL from Teams. Will auto-extract and download the recording without needing organizer_id.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "download_url".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Direct download URL for the recording. Use if you already have it from teams_get_meeting_recording results.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "recording_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The recording ID from teams_get_meeting_recording results. Requires 'organizer_id' for DM/group recordings.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "organizer_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("For DM/group recordings with recording_id: the 'organizer_id' from teams_get_meeting_recording results.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "recording_name".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Optional name of the recording for reference in the transcript.".to_string()),
                            required: false, ..Default::default()
                        },
                    ]
                ),
                (
                    "teams_save_attachment",
                    "Save an image attachment from a Teams message to your uploads folder for later use. Use this when you need to keep an image for reference or processing.",
                    UseExternalServiceToolType::TeamsSaveAttachment,
                    vec![
                        SchemaField {
                            name: "attachment_url".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The URL of the attachment from the message metadata.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "filename".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Name to save the file as (e.g. 'screenshot.png').".to_string()),
                            required: true, ..Default::default()
                        },
                    ]
                ),
                (
                    "teams_describe_image",
                    "Describe an image attachment from a Teams message. Use this when you need to understand what's in an image the user sent. You can optionally specify what to focus on.",
                    UseExternalServiceToolType::TeamsDescribeImage,
                    vec![
                        SchemaField {
                            name: "attachment_url".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The URL of the image attachment from the message metadata.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "prompt".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Optional: What to look for or ask about the image (e.g. 'What text is visible?', 'Is there a chart?', 'What colors are used?').".to_string()),
                            required: false, ..Default::default()
                        },
                    ]
                ),
                (
                    "teams_transcribe_audio",
                    "Transcribe a voice note or audio attachment from a Teams message. Use this when you need to understand what was said in an audio message. For voice notes (contentType=application/vnd.microsoft.card.audio), the URL is inside the attachment's 'content' field as a JSON string - parse it to extract the 'url' property.",
                    UseExternalServiceToolType::TeamsTranscribeAudio,
                    vec![
                        SchemaField {
                            name: "attachment_url".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The audio URL. For voice notes, parse the attachment's 'content' JSON to get the 'url' field. The 'content' field contains a JSON string like: {\"url\": \"https://graph.microsoft.com/...\"}".to_string()),
                            required: true, ..Default::default()
                        },
                    ]
                ),
                (
                    "teams_list_contexts",
                    "List all Teams channels, group chats, and DMs that you have access to. Returns context IDs and titles so you can interact with other conversations using trigger_context or understand your available scope.",
                    UseExternalServiceToolType::TeamsListContexts,
                    vec![
                        SchemaField {
                            name: "limit".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Maximum number of contexts to return (default: 25).".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "offset".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Offset for pagination (default: 0).".to_string()),
                            required: false, ..Default::default()
                        },
                    ]
                ),
            ];

            for (name, desc, service_type, schema) in teams_tools {
                if !current_tools.iter().any(|t| t.name == name) {
                    current_tools.push(AiTool {
                        id: -1,
                        name: name.to_string(),
                        description: Some(desc.to_string()),
                        tool_type: AiToolType::UseExternalService,
                        deployment_id: self.agent.deployment_id,
                        configuration: AiToolConfiguration::UseExternalService(
                            UseExternalServiceToolConfiguration {
                                service_type,
                                input_schema: Some(schema),
                            },
                        ),
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                    });
                }
            }
        }

        if clickup_enabled {
            let clickup_tools: Vec<(&str, &str, UseExternalServiceToolType, Vec<SchemaField>)> = vec![
                (
                    "clickup_get_current_user",
                    "Get the currently authenticated ClickUp user. Returns user ID, username, email, and timezone.",
                    UseExternalServiceToolType::ClickUpGetCurrentUser,
                    vec![]
                ),
                (
                    "clickup_get_teams",
                    "Get all ClickUp teams/workspaces the authenticated user has access to.",
                    UseExternalServiceToolType::ClickUpGetTeams,
                    vec![]
                ),
                (
                    "clickup_get_spaces",
                    "Get all spaces in a ClickUp team/workspace.",
                    UseExternalServiceToolType::ClickUpGetSpaces,
                    vec![
                        SchemaField {
                            name: "team_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The team/workspace ID. Get this from clickup_get_teams.".to_string()),
                            required: true, ..Default::default()
                        }
                    ]
                ),
                (
                    "clickup_get_space_lists",
                    "Get all folderless lists directly in a ClickUp space. Use this when a space has no folders, or to get lists not inside any folder.",
                    UseExternalServiceToolType::ClickUpGetSpaceLists,
                    vec![
                        SchemaField {
                            name: "space_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The space ID. Get this from clickup_get_spaces.".to_string()),
                            required: true, ..Default::default()
                        }
                    ]
                ),
                (
                    "clickup_get_tasks",
                    "Get tasks from a list. Returns task details.",
                    UseExternalServiceToolType::ClickUpGetTasks,
                    vec![
                        SchemaField {
                            name: "list_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The list ID to get tasks from.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "archived".to_string(),
                            field_type: "BOOLEAN".to_string(),
                            description: Some("Include archived tasks (default: false).".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "page".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Page number (default: 0).".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "order_by".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Order by field (created, updated, id, due_date, etc.).".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "reverse".to_string(),
                            field_type: "BOOLEAN".to_string(),
                            description: Some("Reverse order (default: false).".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "subtasks".to_string(),
                            field_type: "BOOLEAN".to_string(),
                            description: Some("Include subtasks (default: false).".to_string()),
                            required: false, ..Default::default()
                        },
                    ]
                ),
                (
                    "clickup_search_tasks",
                    "Search for tasks across a team/workspace using filters. Combine filters like assignees, status, and keywords to find specific work items.",
                    UseExternalServiceToolType::ClickUpSearchTasks,
                    vec![
                        SchemaField {
                            name: "team_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The team/workspace ID to search in. Get this from clickup_get_teams.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "search".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Search keywords to match against task name or description.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "assignees".to_string(),
                            field_type: "ARRAY".to_string(),
                            description: Some("Filter tasks assigned to specific user IDs.".to_string()),
                            required: false,
                            items_type: Some("STRING".to_string()),
                        },
                        SchemaField {
                            name: "statuses".to_string(),
                            field_type: "ARRAY".to_string(),
                            description: Some("Filter by specific status names (e.g., ['Open', 'in progress']).".to_string()),
                            required: false,
                            items_type: Some("STRING".to_string()),
                        },
                        SchemaField {
                            name: "archived".to_string(),
                            field_type: "BOOLEAN".to_string(),
                            description: Some("Include archived tasks (default: false).".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "date_created_gt".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Filter tasks created after this timestamp.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "due_date_gt".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Filter tasks due after this timestamp.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "due_date_lt".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Filter tasks due before this timestamp.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "page".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Page number (default: 0).".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "order_by".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Order by field (created, updated, id, due_date, etc.).".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "reverse".to_string(),
                            field_type: "BOOLEAN".to_string(),
                            description: Some("Reverse order (default: false).".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "subtasks".to_string(),
                            field_type: "BOOLEAN".to_string(),
                            description: Some("Include subtasks (default: false).".to_string()),
                            required: false, ..Default::default()
                        },
                    ]
                ),
                (
                    "clickup_get_task",
                    "Get details of a specific ClickUp task by ID.",
                    UseExternalServiceToolType::ClickUpGetTask,
                    vec![
                        SchemaField {
                            name: "task_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The task ID to retrieve.".to_string()),
                            required: true, ..Default::default()
                        }
                    ]
                ),
                (
                    "clickup_create_task",
                    "Create a new task in a ClickUp list.",
                    UseExternalServiceToolType::ClickUpCreateTask,
                    vec![
                        SchemaField {
                            name: "list_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The list ID where the task will be created. Get this from clickup_get_space_lists.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "name".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The task name/title.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "description".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Task description (supports markdown).".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "assignees".to_string(),
                            field_type: "ARRAY".to_string(),
                            description: Some("Array of user IDs to assign the task to.".to_string()),
                            required: false,
                            items_type: Some("STRING".to_string()),
                        },
                        SchemaField {
                            name: "status".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The status name for the task (e.g., 'to do', 'in progress', 'complete'). This is REQUIRED - check the space settings for valid status names.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "priority".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Task priority (1=urgent, 2=high, 3=normal, 4=low).".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "due_date".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Due date as Unix timestamp in milliseconds.".to_string()),
                            required: false, ..Default::default()
                        },
                    ]
                ),
                (
                    "clickup_create_list",
                    "Create a new list directly in a ClickUp space (folderless list). Use this when you need to create a list for tasks.",
                    UseExternalServiceToolType::ClickUpCreateList,
                    vec![
                        SchemaField {
                            name: "space_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The space ID where the list will be created. Get this from clickup_get_spaces.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "name".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The list name.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "content".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("List description.".to_string()),
                            required: false, ..Default::default()
                        },
                    ]
                ),
                (
                    "clickup_update_task",
                    "Update an existing task. Change status, assignees, priority, due date, or description.",
                    UseExternalServiceToolType::ClickUpUpdateTask,
                    vec![
                        SchemaField {
                            name: "task_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The task ID to update.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "name".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("New task name.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "description".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("New task description (supports markdown).".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "status".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("New status (e.g., 'Open', 'in progress', 'complete').".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "priority".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Priority (1=urgent, 2=high, 3=normal, 4=low).".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "due_date".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Due date as Unix timestamp in milliseconds.".to_string()),
                            required: false, ..Default::default()
                        },
                        SchemaField {
                            name: "assignees".to_string(),
                            field_type: "ARRAY".to_string(),
                            description: Some("Array of user IDs to assign.".to_string()),
                            required: false,
                            items_type: Some("STRING".to_string()),
                        },
                    ]
                ),
                (
                    "clickup_add_comment",
                    "Add a comment to a task. Use for providing updates, notes, or communication on tasks.",
                    UseExternalServiceToolType::ClickUpAddComment,
                    vec![
                        SchemaField {
                            name: "task_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The task ID to add a comment to.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "comment_text".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The comment text to add.".to_string()),
                            required: true, ..Default::default()
                        },
                        SchemaField {
                            name: "notify_all".to_string(),
                            field_type: "BOOLEAN".to_string(),
                            description: Some("Notify all assignees about the comment (default: false).".to_string()),
                            required: false, ..Default::default()
                        },
                    ]
                ),
            ];

            for (name, desc, service_type, schema) in clickup_tools {
                if !current_tools.iter().any(|t| t.name == name) {
                    current_tools.push(AiTool {
                        id: -1,
                        name: name.to_string(),
                        description: Some(desc.to_string()),
                        tool_type: AiToolType::UseExternalService,
                        deployment_id: self.agent.deployment_id,
                        configuration: AiToolConfiguration::UseExternalService(
                            UseExternalServiceToolConfiguration {
                                service_type,
                                input_schema: Some(schema),
                            },
                        ),
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                    });
                }
            }
        }

        if !current_tools
            .iter()
            .any(|t| t.name == "spawn_context_execution")
        {
            current_tools.push(AiTool {
                id: -1,
                name: "spawn_context_execution".to_string(),
                description: Some(
                    "Spawn a new agent execution in another context. Just like you are currently running in your context with your own conversation history, tools, and workflows - this will start a separate, self-contained agent instance in the target context. The spawned instance will receive your message as input and operate independently with its own context, history, and available tools. Use this to delegate tasks, notify other channels, or hand off work.".to_string()
                ),
                tool_type: AiToolType::UseExternalService,
                deployment_id: self.agent.deployment_id,
                configuration: AiToolConfiguration::UseExternalService(
                    UseExternalServiceToolConfiguration {
                        service_type: UseExternalServiceToolType::TriggerContext,
                        input_schema: Some(vec![
                            SchemaField {
                                name: "target_context_id".to_string(),
                                field_type: "STRING".to_string(),
                                description: Some("The context ID where the new agent instance will be spawned.".to_string()),
                                required: true, ..Default::default()
                            },
                            SchemaField {
                                name: "message".to_string(),
                                field_type: "STRING".to_string(),
                                description: Some("The message/task to send to the spawned instance. This becomes the input that the new instance will process.".to_string()),
                                required: true, ..Default::default()
                            },
                            SchemaField {
                                name: "actionable_id".to_string(),
                                field_type: "STRING".to_string(),
                                description: Some("If you are fulfilling an actionable from your pending list, provide its ID here to mark it complete.".to_string()),
                                required: false, ..Default::default()
                            },
                            SchemaField {
                                name: "execute".to_string(),
                                field_type: "BOOLEAN".to_string(),
                                description: Some("Whether to actually spawn the agent instance. Default: true. Set to false to just add the message to the target context's history without triggering execution.".to_string()),
                                required: false, ..Default::default()
                            },
                        ]),
                    }
                ),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            });
        }

        let mut agent_with_tools = self.agent.clone();
        agent_with_tools.tools = current_tools;

        let mut executor = AgentExecutor {
            agent: agent_with_tools.clone(),
            app_state: self.app_state.clone(),
            context_id: self.context_id,
            context_orchestrator,
            tool_executor,
            user_request: String::new(),
            channel: self.channel,
            memories: Vec::new(),
            loaded_memory_ids: std::collections::HashSet::new(),
            conversations: Vec::new(),
            current_objective: None,
            conversation_insights: None,
            task_results: HashMap::new(),
            current_workflow_id: None,
            current_workflow_state: None,
            current_workflow_node_id: None,
            current_workflow_execution_path: Vec::new(),
            system_instructions: None,
            filesystem,
            shell,
            teams_enabled,
            clickup_enabled,
            context_title: self.context_title,
        };

        executor.system_instructions = context.system_instructions.clone();

        if context.status == ExecutionContextStatus::WaitingForInput {
            if let Some(state) = context.execution_state {
                executor.restore_from_state(state)?;
            }
        }

        Ok(executor)
    }
}

impl AgentExecutor {
    pub async fn new(
        agent: AiAgentWithFeatures,
        context_id: i64,
        app_state: AppState,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
        context_title: String,
    ) -> Result<Self, AppError> {
        AgentExecutorBuilder::new(agent, context_id, app_state, channel, context_title)
            .build()
            .await
    }

    pub(super) fn restore_from_state(
        &mut self,
        state: AgentExecutionState,
    ) -> Result<(), AppError> {
        self.task_results = state
            .task_results
            .into_iter()
            .filter_map(|(k, v)| {
                serde_json::from_value::<TaskExecutionResult>(v)
                    .ok()
                    .map(|result| (k, result))
            })
            .collect();

        if let Some(objective) = state.current_objective {
            self.current_objective = serde_json::from_value(objective).ok();
        }

        if let Some(insights) = state.conversation_insights {
            self.conversation_insights = serde_json::from_value(insights).ok();
        }

        if let Some(workflow_state) = state.workflow_state {
            self.current_workflow_id = Some(workflow_state.workflow_id);
            self.current_workflow_state = Some(workflow_state.workflow_state);
            self.current_workflow_node_id = Some(workflow_state.current_node_id);
            self.current_workflow_execution_path = workflow_state.execution_path;
        }

        Ok(())
    }

    pub(super) fn get_current_workflow_state(&self) -> Option<WorkflowExecutionState> {
        match (
            self.current_workflow_id,
            &self.current_workflow_state,
            &self.current_workflow_node_id,
        ) {
            (Some(workflow_id), Some(workflow_state), Some(node_id)) => {
                Some(WorkflowExecutionState {
                    workflow_id,
                    workflow_state: workflow_state.clone(),
                    current_node_id: node_id.clone(),
                    execution_path: self.current_workflow_execution_path.clone(),
                })
            }
            _ => None,
        }
    }
}
