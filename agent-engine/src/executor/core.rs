use crate::context::ContextOrchestrator;
use crate::tools::ToolExecutor;
use crate::filesystem::{AgentFilesystem, shell::ShellExecutor};

use common::error::AppError;
use common::state::AppState;
use dto::json::agent_executor::{
    ConversationInsights, ObjectiveDefinition, TaskExecutionResult,
};
use dto::json::StreamEvent;
use models::{
    AgentExecutionState, AiAgentWithFeatures, ConversationRecord, ExecutionContextStatus, MemoryRecord, WorkflowExecutionState,
};
use models::{AiTool, AiToolConfiguration, AiToolType, InternalToolConfiguration, InternalToolType, SchemaField, UseExternalServiceToolConfiguration, UseExternalServiceToolType};
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
}

pub struct AgentExecutorBuilder {
    agent: AiAgentWithFeatures,
    app_state: AppState,
    context_id: i64,
    channel: tokio::sync::mpsc::Sender<StreamEvent>,
}

impl AgentExecutorBuilder {
    pub fn new(
        agent: AiAgentWithFeatures,
        context_id: i64,
        app_state: AppState,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Self {
        Self {
            agent,
            context_id,
            app_state,
            channel,
        }
    }

    pub async fn build(self) -> Result<AgentExecutor, AppError> {
        let tool_executor =
            ToolExecutor::new(self.app_state.clone(), self.agent.clone(), self.context_id).with_channel(self.channel.clone());
        let context_orchestrator =
            ContextOrchestrator::new(self.app_state.clone(), self.agent.clone(), self.context_id);

        let execution_id = self.app_state.sf.next_id()?.to_string();
        
        let filesystem = AgentFilesystem::new(
            &self.agent.deployment_id.to_string(),
            &self.context_id.to_string(),
            &execution_id,
        );
        
        if let Err(e) = filesystem.initialize().await {
            tracing::warn!("Failed to initialize agent filesystem: {}", e);
        }

        let shell = ShellExecutor::new(filesystem.execution_root());
        
        for kb in &self.agent.knowledge_bases {
            if let Err(e) = filesystem.link_knowledge_base(&kb.id.to_string(), &kb.name).await {
                tracing::warn!("Failed to link knowledge base {} ({}): {}", kb.name, kb.id, e);
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
                        required: true,
                    },
                    SchemaField {
                        name: "start_line".to_string(),
                        field_type: "INTEGER".to_string(),
                        description: Some("Start line (1-indexed, optional)".to_string()),
                        required: false,
                    },
                    SchemaField {
                        name: "end_line".to_string(),
                        field_type: "INTEGER".to_string(),
                        description: Some("End line (inclusive, optional)".to_string()),
                        required: false,
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
                        required: true,
                    },
                    SchemaField {
                        name: "content".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Content to write".to_string()),
                        required: true,
                    },
                    SchemaField {
                        name: "start_line".to_string(),
                        field_type: "INTEGER".to_string(),
                        description: Some("Replace from this line (1-indexed). Requires prior read_file.".to_string()),
                        required: false,
                    },
                    SchemaField {
                        name: "end_line".to_string(),
                        field_type: "INTEGER".to_string(),
                        description: Some("Replace up to this line (inclusive). Requires prior read_file.".to_string()),
                        required: false,
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
                        required: false,
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
                        required: true,
                    },
                    SchemaField {
                        name: "path".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Directory to search (default: '/')".to_string()),
                        required: false,
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
                        required: true,
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
                        required: true,
                    },
                    SchemaField {
                        name: "category".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Category: procedural (how-to), semantic (facts), episodic (events), working (temp)".to_string()),
                        required: true,
                    },
                    SchemaField {
                        name: "importance".to_string(),
                        field_type: "NUMBER".to_string(),
                        description: Some("Importance 0.0-1.0 (default: 0.5)".to_string()),
                        required: false,
                    }
                ]
            ),
        ];

        // Check for active Teams integrations to enable proactive messaging tools
        // 1. Get the context to find the context_group (moved up from below)
        let context = GetExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
            .execute(&self.app_state)
            .await?;

        let mut teams_enabled = false;

        if let Some(context_group) = &context.context_group {
            let active_integrations = queries::GetActiveIntegrationsForContextQuery::new(
                self.agent.deployment_id,
                self.agent.id,
                context_group.clone()
            ).execute(&self.app_state).await?;

            let has_teams = active_integrations.iter().any(|i| matches!(i.integration_type, models::IntegrationType::Teams));

            if has_teams {
                teams_enabled = true;
                tracing::info!("Context group {} has active Teams integration. Injecting Teams tools.", context_group);
                
                // Symlink teams-activity to agent's virtual filesystem
                if let Err(e) = filesystem.link_teams_activity(context_group).await {
                    tracing::warn!("Failed to link teams-activity directory: {}", e);
                }
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
                    configuration: AiToolConfiguration::Internal(
                        InternalToolConfiguration {
                            tool_type,
                            input_schema: Some(schema),
                        }
                    ),
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                });
            }
        }
        
        // Add Teams external service tools if enabled
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
                            required: false,
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
                            required: true,
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
                            required: true,
                        },
                        SchemaField {
                            name: "message".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The message content to send. MUST include context about who is asking if relaying on behalf of someone.".to_string()),
                            required: true,
                        },
                        SchemaField {
                            name: "sender_info".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("REQUIRED: Who is sending this and from where. Example: 'Saurav Singh from #General channel' or 'John Smith from group DM'. This is prepended to your message for clarity.".to_string()),
                            required: true,
                        },
                        SchemaField {
                            name: "notify_on_reply".to_string(),
                            field_type: "BOOLEAN".to_string(),
                            description: Some("If true, you will be notified in your current context when the user replies. Default: false.".to_string()),
                            required: false,
                        },
                        SchemaField {
                            name: "description".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Context for why you're sending this DM - helps when processing the reply. Required if notify_on_reply is true.".to_string()),
                            required: false,
                        },
                        SchemaField {
                            name: "context_notes".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Transient memory passed to the DM context. Include: WHO is asking (name), WHAT they need, and any context. The agent handling this DM sees these notes and can act accordingly. Example: 'John Smith asked for the project deadline - let him know when you get a response'.".to_string()),
                            required: false,
                        },
                    ]
                ),
                (
                    "teams_get_current_channel_messages",
                    "Get recent messages from your current Teams conversation. Works for Team channels, group DMs, and 1:1 chats. Useful for finding meeting notifications, previous discussions, or context.",
                    UseExternalServiceToolType::TeamsGetChannelMessages,
                    vec![
                        SchemaField {
                            name: "count".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Number of messages to fetch (default: 20, max: 50).".to_string()),
                            required: false,
                        },
                        SchemaField {
                            name: "before_timestamp".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("ISO timestamp for pagination - fetch messages before this time.".to_string()),
                            required: false,
                        },
                    ]
                ),
                (
                    "teams_get_meeting_recording",
                    "Get meeting recordings. For DM/group chats: uses organizer_id to search their OneDrive. For channel meetings: automatically detects Team context and searches SharePoint. Set search_recent=true when you don't have a meeting ID.",
                    UseExternalServiceToolType::TeamsGetMeetingRecording,
                    vec![
                        SchemaField {
                            name: "organizer_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("For DM/group chat recordings. The AAD Object ID of the meeting organizer from callEnded events. Not needed for channel meetings (auto-detected).".to_string()),
                            required: false,
                        },
                        SchemaField {
                            name: "meeting_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The meeting ID. Optional if using search_recent.".to_string()),
                            required: false,
                        },
                        SchemaField {
                            name: "join_url".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The meeting join URL. Optional if using search_recent.".to_string()),
                            required: false,
                        },
                        SchemaField {
                            name: "search_recent".to_string(),
                            field_type: "BOOLEAN".to_string(),
                            description: Some("Set to true to search for recent recordings without needing meeting ID. Useful for ad-hoc calls.".to_string()),
                            required: false,
                        },
                        SchemaField {
                            name: "max_results".to_string(),
                            field_type: "INTEGER".to_string(),
                            description: Some("Maximum number of recordings to return. Default 5.".to_string()),
                            required: false,
                        },
                    ]
                ),
                (
                    "teams_analyze_meeting",
                    "Analyze a meeting recording. Downloads video and extracts: 1) Audio transcription with speaker labels, 2) Visual content. For DM recordings, pass organizer_id from teams_get_meeting_recording results.",
                    UseExternalServiceToolType::TeamsTranscribeMeeting,
                    vec![
                        SchemaField {
                            name: "recording_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The recording ID from teams_get_meeting_recording results (the 'id' field).".to_string()),
                            required: true,
                        },
                        SchemaField {
                            name: "organizer_id".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("For DM/group recordings ONLY: the 'organizer_id' from teams_get_meeting_recording results. Not needed for channel meetings.".to_string()),
                            required: false,
                        },
                        SchemaField {
                            name: "recording_name".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Optional name of the recording for reference.".to_string()),
                            required: false,
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
                            required: true,
                        },
                        SchemaField {
                            name: "filename".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("Name to save the file as (e.g. 'screenshot.png').".to_string()),
                            required: true,
                        },
                    ]
                ),
                (
                    "teams_describe_image",
                    "Describe an image attachment from a Teams message. Use this when you need to understand what's in an image the user sent.",
                    UseExternalServiceToolType::TeamsDescribeImage,
                    vec![
                        SchemaField {
                            name: "attachment_url".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The URL of the image attachment from the message metadata.".to_string()),
                            required: true,
                        },
                    ]
                ),
                (
                    "teams_transcribe_audio",
                    "Transcribe a voice note or audio attachment from a Teams message. Use this when you need to understand what was said in an audio message.",
                    UseExternalServiceToolType::TeamsTranscribeAudio,
                    vec![
                        SchemaField {
                            name: "attachment_url".to_string(),
                            field_type: "STRING".to_string(),
                            description: Some("The URL of the audio attachment from the message metadata.".to_string()),
                            required: true,
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
                            }
                        ),
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                    });
                }
            }
        }
        
        // Add trigger_context tool (always available for cross-context messaging)
        if !current_tools.iter().any(|t| t.name == "trigger_context") {
            current_tools.push(AiTool {
                id: -1,
                name: "trigger_context".to_string(),
                description: Some("Trigger execution in another context with a message. Use this to relay information or notify another conversation.".to_string()),
                tool_type: AiToolType::UseExternalService,
                deployment_id: self.agent.deployment_id,
                configuration: AiToolConfiguration::UseExternalService(
                    UseExternalServiceToolConfiguration {
                        service_type: UseExternalServiceToolType::TriggerContext,
                        input_schema: Some(vec![
                            SchemaField {
                                name: "target_context_id".to_string(),
                                field_type: "STRING".to_string(),
                                description: Some("The context ID to send the message to (as string to preserve precision).".to_string()),
                                required: true,
                            },
                            SchemaField {
                                name: "message".to_string(),
                                field_type: "STRING".to_string(),
                                description: Some("The message to inject into the target context.".to_string()),
                                required: true,
                            },
                            SchemaField {
                                name: "actionable_id".to_string(),
                                field_type: "STRING".to_string(),
                                description: Some("CRITICAL: When fulfilling an actionable, you MUST provide its ID here. This removes it from your pending list. Without this, the actionable will persist forever. Get the ID from the actionables shown in system context (e.g., 'notify_1736...').".to_string()),
                                required: false,
                            },
                            SchemaField {
                                name: "execute".to_string(),
                                field_type: "BOOLEAN".to_string(),
                                description: Some("Whether to trigger agent execution in target context. Default: true. Set to false to just add message without execution.".to_string()),
                                required: false,
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
        };

        executor.system_instructions = context.system_instructions.clone();

        if teams_enabled {
            // Teams instructions should be configured in the agent's system instructions template
            // using variables like {{deployment_id}}, {{context_group}}, etc.
            tracing::debug!("Teams integration enabled for context {}", self.context_id);
        }

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
    ) -> Result<Self, AppError> {
        AgentExecutorBuilder::new(agent, context_id, app_state, channel)
            .build()
            .await
    }

    pub(super) fn restore_from_state(&mut self, state: AgentExecutionState) -> Result<(), AppError> {
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
