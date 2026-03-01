//! Tool definitions for the agent executor.
//!
//! This module contains all the static tool schemas injected into the agent
//! for internal tools, Teams integration, and ClickUp integration.

use models::{InternalToolType, SchemaField, UseExternalServiceToolType};

/// Internal filesystem and execution tools
pub fn internal_tools() -> Vec<(
    &'static str,
    &'static str,
    InternalToolType,
    Vec<SchemaField>,
)> {
    vec![
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
                    ..Default::default()
                },
                SchemaField {
                    name: "start_line".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Start line (1-indexed, optional)".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "end_line".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("End line (inclusive, optional)".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "read_image",
            "Read an image file and return mime metadata plus base64 payload for one-time vision analysis.",
            InternalToolType::ReadImage,
            vec![SchemaField {
                name: "path".to_string(),
                field_type: "STRING".to_string(),
                description: Some("Path to image file (e.g. /uploads/photo.png)".to_string()),
                required: true,
                ..Default::default()
            }],
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
                    ..Default::default()
                },
                SchemaField {
                    name: "content".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Content to write".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "start_line".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Replace from this line (1-indexed). Requires prior read_file.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "end_line".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Replace up to this line (inclusive). Requires prior read_file.".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "list_directory",
            "List files and directories at a path.",
            InternalToolType::ListDirectory,
            vec![SchemaField {
                name: "path".to_string(),
                field_type: "STRING".to_string(),
                description: Some("Directory path (default: '/')".to_string()),
                required: false,
                ..Default::default()
            }],
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
                    ..Default::default()
                },
                SchemaField {
                    name: "path".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Directory to search (default: '/')".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "execute_command",
            "Execute a shell command. Allowed commands: cat, head, tail, grep, rg, find, ls, tree, wc, du, df, touch, mkdir, echo, cp, mv, rm, chmod, sed, awk, sort, uniq, jq, cut, tr, diff, date, whoami, pwd, printf, python, python3.",
            InternalToolType::ExecuteCommand,
            vec![SchemaField {
                name: "command".to_string(),
                field_type: "STRING".to_string(),
                description: Some("Shell command to run".to_string()),
                required: true,
                ..Default::default()
            }],
        ),
        (
            "sleep",
            "Pause execution briefly. Use when waiting for external updates or when no immediate action is required.",
            InternalToolType::Sleep,
            vec![
                SchemaField {
                    name: "duration_ms".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Sleep duration in milliseconds (max 10000).".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "reason".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional brief reason for the wait.".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "switch_execution_mode",
            "Switch execution mode from normal mode only. Supported modes: 'supervisor' (orchestration-only) and 'long_think_and_reason' (high-quality reasoning for next decision only).",
            InternalToolType::SwitchExecutionMode,
            vec![
                SchemaField {
                    name: "mode".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Target mode: 'supervisor' or 'long_think_and_reason'.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "reason".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional reason for mode switch.".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "update_task_board",
            "Supervisor-only: add or update in-memory task board entries for delegated work tracking.",
            InternalToolType::UpdateTaskBoard,
            vec![
                SchemaField {
                    name: "task_id".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Stable task identifier in the supervisor board.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "title".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Task title.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "status".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Task status (queued|in_progress|blocked|completed|failed).".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "owner_agent".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Assigned sub-agent name.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "context_id".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Related child/execution context ID.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "notes".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional notes/update.".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "exit_supervisor_mode",
            "Supervisor-only: exit supervisor role and restore normal tool access when delegation is complete.",
            InternalToolType::ExitSupervisorMode,
            vec![SchemaField {
                name: "reason".to_string(),
                field_type: "STRING".to_string(),
                description: Some("Optional reason for exiting supervisor mode.".to_string()),
                required: false,
                ..Default::default()
            }],
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
                    ..Default::default()
                },
                SchemaField {
                    name: "category".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Category: procedural (how-to), semantic (facts), episodic (events), working (temp)".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "importance".to_string(),
                    field_type: "NUMBER".to_string(),
                    description: Some("Importance 0.0-1.0 (default: 0.5)".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "update_status",
            "Post a brief status update that your parent agent can see. Use this to communicate progress on your work. Keep updates concise - one line.",
            InternalToolType::UpdateStatus,
            vec![
                SchemaField {
                    name: "status".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Brief status update (e.g., 'Processing row 500 of 1000...', 'Waiting for user input', 'Generated 3 charts')".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "metadata".to_string(),
                    field_type: "OBJECT".to_string(),
                    description: Some("Optional structured data (e.g., progress percentage, step numbers)".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "get_child_status",
            "Check the status of any child agents you have spawned. Returns current status, latest update, and completion summary if done.",
            InternalToolType::GetChildStatus,
            vec![SchemaField {
                name: "include_completed".to_string(),
                field_type: "BOOLEAN".to_string(),
                description: Some("Include children that have already completed or failed".to_string()),
                required: false,
                ..Default::default()
            }],
        ),
        (
            "spawn_control",
            "Send control commands to a spawned child agent: stop, restart, or update parameters.",
            InternalToolType::SpawnControl,
            spawn_control_schema(),
        ),
        (
            "get_completion_summary",
            "Get completion summaries from your child agents. Results are available after children finish executing.",
            InternalToolType::GetCompletionSummary,
            get_completion_summary_schema(),
        ),
        (
            "notify_parent",
            "Send a message to your parent agent. Only available when you are a child agent (spawned by a parent). Use this to report findings, ask for guidance, or relay important information mid-execution.",
            InternalToolType::NotifyParent,
            vec![
                SchemaField {
                    name: "message".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("The message to send to the parent agent".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "trigger_execution".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("If true, the parent agent will be triggered to process this message immediately. Default: false (message is queued for next parent iteration)".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "get_child_messages",
            "Retrieve messages sent by your child agents via notify_parent. Call this during your supervisor polling loop to check if any children have sent you messages. Only returns messages received since your current execution started.",
            InternalToolType::GetChildMessages,
            vec![],
        ),
    ]
}

/// Microsoft Teams integration tools
pub fn teams_tools() -> Vec<(
    &'static str,
    &'static str,
    UseExternalServiceToolType,
    Vec<SchemaField>,
)> {
    vec![
        (
            "teams_list_users",
            "List users in the Microsoft Teams tenant. Returns up to 25 users by default.",
            UseExternalServiceToolType::TeamsListUsers,
            vec![SchemaField {
                name: "limit".to_string(),
                field_type: "INTEGER".to_string(),
                description: Some("Max number of users to return (default: 25).".to_string()),
                required: false,
                ..Default::default()
            }],
        ),
        (
            "teams_search_users",
            "Search for users in the Microsoft Teams tenant by name or email.",
            UseExternalServiceToolType::TeamsSearchUsers,
            vec![SchemaField {
                name: "query".to_string(),
                field_type: "STRING".to_string(),
                description: Some("Search query (name or email).".to_string()),
                required: true,
                ..Default::default()
            }],
        ),
        (
            "teams_list_messages",
            "Get recent messages from a Teams conversation. Works for Team channels, group DMs, and 1:1 chats. Useful for finding meeting notifications, previous discussions, or context. Use context_id to read from another channel/chat. NOTE: Pinned/Inline images appear as <img> tags in the body HTML, not always as attachments - look for 'src' attributes.",
            UseExternalServiceToolType::TeamsListMessages,
            vec![
                SchemaField {
                    name: "context_id".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional: Target context ID to read messages from. Use teams_list_conversations() to discover available contexts. Defaults to current context.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "count".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Batch size for fetching. If `from_date` is used, this acts as page size (system auto-paginates up to 500 messages). If no date, this is the limit (max 50).".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "from_date".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("ISO date/datetime to filter messages FROM (inclusive). Example: '2026-01-01' or '2026-01-01T09:00:00Z'.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "to_date".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("ISO date/datetime to filter messages TO (inclusive). Example: '2026-01-10' or '2026-01-10T18:00:00Z'.".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "teams_get_meeting_recording",
            "Get meeting recording info. PREFERRED: Pass 'join_url' from a meeting recap link to auto-extract recording. ALTERNATIVE: For channel meetings team_id is auto-detected. For DM/group chats use 'organizer_id' from callEnded events.",
            UseExternalServiceToolType::TeamsGetMeetingRecording,
            vec![
                SchemaField {
                    name: "context_id".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional: Target context ID. Use teams_list_conversations() to discover available contexts. Defaults to current context.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "organizer_id".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("For DM/group chat recordings. The AAD Object ID of the meeting organizer from callEnded events. Not needed for channel meetings (auto-detected).".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "meeting_id".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("The meeting ID. Optional if using search_recent.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "join_url".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("The meeting join URL. Optional if using search_recent.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "search_recent".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("Set to true to search for recent recordings without needing meeting ID. Useful for ad-hoc calls.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "max_results".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Maximum number of recordings to return. Default 5.".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
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
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "download_url".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Direct download URL for the recording. Use if you already have it from teams_get_meeting_recording results.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "recording_id".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("The recording ID from teams_get_meeting_recording results. Requires 'organizer_id' for DM/group recordings.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "organizer_id".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("For DM/group recordings with recording_id: the 'organizer_id' from teams_get_meeting_recording results.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "recording_name".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional name of the recording for reference in the transcript.".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "teams_save_attachment",
            "Save a file attachment (document, image, etc.) from a Teams message to your uploads folder for later use. Use this when you need to keep a file for reference or processing. Supports any file type. NOTE: For inline images (pasted in chat), extract the URL from the <img> 'src' attribute in the message body HTML.",
            UseExternalServiceToolType::TeamsSaveAttachment,
            vec![
                SchemaField {
                    name: "attachment_url".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("The URL of the attachment from the message metadata.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "filename".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Name to save the file as (e.g. 'screenshot.png').".to_string()),
                    required: true,
                    ..Default::default()
                },
            ],
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
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "prompt".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional: What to look for or ask about the image (e.g. 'What text is visible?', 'Is there a chart?', 'What colors are used?').".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "teams_transcribe_audio",
            "Transcribe a voice note or audio attachment from a Teams message. Use this when you need to understand what was said in an audio message. For voice notes (contentType=application/vnd.microsoft.card.audio), the URL is inside the attachment's 'content' field as a JSON string - parse it to extract the 'url' property.",
            UseExternalServiceToolType::TeamsTranscribeAudio,
            vec![SchemaField {
                name: "attachment_url".to_string(),
                field_type: "STRING".to_string(),
                description: Some("The audio URL. For voice notes, parse the attachment's 'content' JSON to get the 'url' field. The 'content' field contains a JSON string like: {\"url\": \"https://graph.microsoft.com/...\"}".to_string()),
                required: true,
                ..Default::default()
            }],
        ),
        (
            "teams_list_conversations",
            "List all Teams channels, group chats, and DMs that you have access to. Returns context IDs and titles so you can interact with other conversations using spawn_context_execution or understand your available scope.",
            UseExternalServiceToolType::TeamsListContexts,
            vec![
                SchemaField {
                    name: "limit".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Maximum number of contexts to return (default: 25).".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "offset".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Offset for pagination (default: 0).".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
    ]
}

/// ClickUp integration tools
pub fn clickup_tools() -> Vec<(
    &'static str,
    &'static str,
    UseExternalServiceToolType,
    Vec<SchemaField>,
)> {
    vec![
        (
            "clickup_get_current_user",
            "Get the currently authenticated ClickUp user. Returns user ID, username, email, and timezone.",
            UseExternalServiceToolType::ClickUpGetCurrentUser,
            vec![],
        ),
        (
            "clickup_get_teams",
            "Get all ClickUp teams/workspaces the authenticated user has access to.",
            UseExternalServiceToolType::ClickUpGetTeams,
            vec![],
        ),
        (
            "clickup_get_spaces",
            "Get all spaces in a ClickUp team/workspace.",
            UseExternalServiceToolType::ClickUpGetSpaces,
            vec![SchemaField {
                name: "team_id".to_string(),
                field_type: "STRING".to_string(),
                description: Some("The team/workspace ID. Get this from clickup_get_teams.".to_string()),
                required: true,
                ..Default::default()
            }],
        ),
        (
            "clickup_get_space_lists",
            "Get all folderless lists directly in a ClickUp space. Use this when a space has no folders, or to get lists not inside any folder.",
            UseExternalServiceToolType::ClickUpGetSpaceLists,
            vec![SchemaField {
                name: "space_id".to_string(),
                field_type: "STRING".to_string(),
                description: Some("The space ID. Get this from clickup_get_spaces.".to_string()),
                required: true,
                ..Default::default()
            }],
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
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "archived".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("Include archived tasks (default: false).".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "page".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Page number (default: 0).".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "order_by".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Order by field (created, updated, id, due_date, etc.).".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "reverse".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("Reverse order (default: false).".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "subtasks".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("Include subtasks (default: false).".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
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
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "search".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Search keywords to match against task name or description.".to_string()),
                    required: false,
                    ..Default::default()
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
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "date_created_gt".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Filter tasks created after this timestamp.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "due_date_gt".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Filter tasks due after this timestamp.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "due_date_lt".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Filter tasks due before this timestamp.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "page".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Page number (default: 0).".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "order_by".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Order by field (created, updated, id, due_date, etc.).".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "reverse".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("Reverse order (default: false).".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "subtasks".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("Include subtasks (default: false).".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "clickup_get_task",
            "Get details of a specific ClickUp task by ID.",
            UseExternalServiceToolType::ClickUpGetTask,
            vec![SchemaField {
                name: "task_id".to_string(),
                field_type: "STRING".to_string(),
                description: Some("The task ID to retrieve.".to_string()),
                required: true,
                ..Default::default()
            }],
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
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "name".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("The task name/title.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "description".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Task description (supports markdown).".to_string()),
                    required: false,
                    ..Default::default()
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
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "priority".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Task priority (1=urgent, 2=high, 3=normal, 4=low).".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "due_date".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Due date as Unix timestamp in milliseconds.".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
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
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "name".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("The list name.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "content".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("List description.".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
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
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "name".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("New task name.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "description".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("New task description (supports markdown).".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "status".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("New status (e.g., 'Open', 'in progress', 'complete').".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "priority".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Priority (1=urgent, 2=high, 3=normal, 4=low).".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "due_date".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Due date as Unix timestamp in milliseconds.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "assignees".to_string(),
                    field_type: "ARRAY".to_string(),
                    description: Some("Array of user IDs to assign.".to_string()),
                    required: false,
                    items_type: Some("STRING".to_string()),
                },
            ],
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
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "comment_text".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("The comment text to add.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "notify_all".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("Notify all assignees about the comment (default: false).".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "clickup_task_add_attachment",
            "Upload a file from the agent filesystem as an attachment to a ClickUp task. The file can be any image, document, or other file from /uploads/, /workspace/, or /scratch/ directories.",
            UseExternalServiceToolType::ClickUpTaskAddAttachment,
            vec![
                SchemaField {
                    name: "task_id".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("The task ID to attach the file to.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "file_path".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Path to the file in the agent filesystem (e.g., /uploads/screenshot.png, /workspace/report.pdf).".to_string()),
                    required: true,
                    ..Default::default()
                },
            ],
        ),
    ]
}

pub fn mcp_name_slug(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut prev_underscore = false;
    for ch in value.chars() {
        let lower = ch.to_ascii_lowercase();
        let is_valid = lower.is_ascii_alphanumeric();
        if is_valid {
            out.push(lower);
            prev_underscore = false;
        } else if !prev_underscore {
            out.push('_');
            prev_underscore = true;
        }
    }

    out.trim_matches('_').to_string()
}

pub fn mcp_dynamic_tool_name(server_name: &str, mcp_tool_name: &str) -> String {
    let server_slug = mcp_name_slug(server_name);
    let tool_slug = mcp_name_slug(mcp_tool_name);
    format!("mcp__{}__{}", server_slug, tool_slug)
}

/// Schema for spawn_context_execution tool
pub fn spawn_context_execution_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "mode".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Execution mode: 'spawn' (default) creates/uses a target context; 'fork' hands off to another agent in the current context.".to_string()),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "target_context_id".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Optional target context ID where execution should run. If omitted, a temporary child context is created.".to_string()),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "agent_name".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Agent to execute: 'self' or one of your configured sub-agent names.".to_string()),
            required: true,
            ..Default::default()
        },
        SchemaField {
            name: "instructions".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Instruction payload to send to the target context execution.".to_string()),
            required: true,
            ..Default::default()
        },
        SchemaField {
            name: "message".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Deprecated alias for instructions. Use `instructions`.".to_string()),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "execute".to_string(),
            field_type: "BOOLEAN".to_string(),
            description: Some("Whether to trigger immediate execution in the target context. Default: true. If false, just adds the message without executing.".to_string()),
            required: false,
            ..Default::default()
        },
    ]
}

/// Schema for spawn_control tool
pub fn spawn_control_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "child_context_id".to_string(),
            field_type: "STRING".to_string(),
            description: Some("ID of the child context to send control command to.".to_string()),
            required: true,
            ..Default::default()
        },
        SchemaField {
            name: "action".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Control action: 'stop', 'restart', or 'update_params'.".to_string()),
            required: true,
            ..Default::default()
        },
        SchemaField {
            name: "params".to_string(),
            field_type: "OBJECT".to_string(),
            description: Some("For 'update_params' action: the new parameters to set.".to_string()),
            required: false,
            ..Default::default()
        },
    ]
}

/// Schema for get_completion_summary tool
pub fn get_completion_summary_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "child_context_id".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Optional: Specific child context ID to get summary for. If not provided, returns summaries for all completed children.".to_string()),
            required: false,
            ..Default::default()
        },
    ]
}
