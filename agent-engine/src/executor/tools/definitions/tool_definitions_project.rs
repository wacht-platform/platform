use super::tool_definitions_common::string_enum;
use models::{InternalToolType, SchemaField};

fn project_task_assignments_schema() -> SchemaField {
    SchemaField {
        name: "assignments".to_string(),
        field_type: "ARRAY".to_string(),
        description: Some("Ordered assignment chain. List order = stage order: item 1 is the active stage, later items run after it. Each item: thread_id OR a reusable-lane selector (responsibility / capability_tags), plus assignment_role, status, instructions. Idempotent — re-passing the same plan is a no-op; only differing stages mutate. On a stage's success the backend auto-activates the next pending stage.".to_string()),
        min_items: Some(1),
        items_schema: Some(Box::new(SchemaField {
            field_type: "OBJECT".to_string(),
            properties: Some(vec![
                SchemaField {
                    name: "thread_id".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional specific existing thread ID to assign.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "responsibility".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional lane responsibility selector (assign by lane role instead of thread ID).".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "capability_tags".to_string(),
                    field_type: "ARRAY".to_string(),
                    items_type: Some("STRING".to_string()),
                    description: Some("Optional lane capability tags for assignment matching.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "assignment_role".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Stage role.".to_string()),
                    enum_values: string_enum(&[
                        "executor",
                        "reviewer",
                        "specialist_reviewer",
                        "approver",
                        "observer",
                    ]),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "status".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional initial status. Usually omit — runtime defaults: stage 1 `available`, later stages `pending`. Set only to override staged routing.".to_string()),
                    enum_values: string_enum(&[
                        "pending",
                        "available",
                        "claimed",
                        "in_progress",
                        "completed",
                        "rejected",
                        "blocked",
                        "cancelled",
                    ]),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "instructions".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Verbose, self-contained brief for this stage — the only context that crosses from coordinator to assignee (their history is scoped to their own thread). Cover: what to produce, where inputs/outputs live, all constraints, inherited artifacts/decisions, acceptance criteria, current deliverable state, prior blockers. Terse briefs cause re-work. Required for executor and review stages.".to_string()),
                    required: false,
                    ..Default::default()
                },
            ]),
            ..Default::default()
        })),
        required: true,
        ..Default::default()
    }
}

pub(crate) fn project_tools() -> Vec<(
    &'static str,
    &'static str,
    InternalToolType,
    Vec<SchemaField>,
)> {
    vec![
        (
            "create_project_task",
            "Create a task on the shared project board (coordinator and user-facing conversation threads). The runtime generates the task key. When a conversation thread creates a task it is auto-routed to the coordinator. Use for durable delegated/background/async work that should continue while this thread stays focused. Write `description` as a direct, sequenced instruction (\"First X. Then Y. Finally Z.\"), not commentary. Optionally nest under `parent_task_key`.",
            InternalToolType::CreateProjectTask,
            create_project_task_schema(),
        ),
        (
            "update_project_task",
            "Update an existing board task by key. Coordinator/execution lanes use it for status, schedule, terminal transitions. Conversation threads only touch `title`/`description` (revise the brief; never change status — cancel/complete are coordinator decisions); editing either preempts any running execution and re-routes the coordinator with the new instructions. Write `description` as a direct, sequenced instruction. Omit unchanged fields.",
            InternalToolType::UpdateProjectTask,
            update_project_task_schema(),
        ),
        (
            "assign_project_task",
            "Replace the assignment plan for an existing board task by key. Coordinator-only. Routes work through execution and review lanes.",
            InternalToolType::AssignProjectTask,
            assign_project_task_schema(),
        ),
        (
            "list_threads",
            "List threads in the current project so work can be assigned through the task board against real lanes.",
            InternalToolType::ListThreads,
            list_threads_schema(),
        ),
        (
            "create_thread",
            "Create a durable execution lane in the current project. Coordinator-only. `assigned_agent_name` is REQUIRED (no default) — almost always a specialist from `available_sub_agents` whose responsibility matches the lane; pick yourself only for genuinely coordinator-owned execution (rare — coordinators delegate, not execute). Per-task delegation happens via project-task assignments after the lane exists.",
            InternalToolType::CreateThread,
            create_thread_schema(),
        ),
        (
            "update_thread",
            "Update an existing durable lane in the current project. Coordinator-only. Change title, responsibility, instructions, or assignment capability before routing work.",
            InternalToolType::UpdateThread,
            update_thread_schema(),
        ),
        (
            "subscribe_to_task",
            "Subscribe this conversation thread to a project task's status-change notifications. Conversation threads only. Defaults to `completed`/`blocked`/`cancelled`; narrow via `event_kinds`. `create_project_task` auto-subscribes unless `auto_subscribe: false` — use this only for tasks the thread didn't create or to broaden the kinds.",
            InternalToolType::SubscribeToTask,
            subscribe_to_task_schema(),
        ),
        (
            "unsubscribe_from_task",
            "Stop receiving status-change notifications for a project task. Conversation threads only.",
            InternalToolType::UnsubscribeFromTask,
            unsubscribe_from_task_schema(),
        ),
        (
            "delegate_task",
            "Hand a discrete piece of work to an existing execution lane. Conversation threads only; the lane's agent owns it exclusively (coordinator/reviewer don't see it). Give clear boundaries in `description`: what to inspect, what to ignore, the exact deliverable to write. For folder analysis, pass `input_mounts` for read-only views of `/workspace/<folder>` at `/delegated_inputs/<alias>/`.\n\nShared output workspace (same S3 prefix, two mounts): you read/write at `/workspace/delegate/<task_key>/`; the lane reads/writes at `/delegated_workspace/`. Lane outputs appear in YOUR `/workspace/delegate/<task_key>/` — the only place to find them. Do NOT look under `/project_workspace/tasks/<task_key>/` (coordinator-side TASK.md/JOURNAL.md only; empty of delegated artifacts).\n\nStatus auto-completes when the lane finishes (no coordinator/reviewer step). You're auto-subscribed to status updates.",
            InternalToolType::DelegateTask,
            delegate_task_schema(),
        ),
        (
            "get_project_task",
            "Get current status, schedule, and latest assignment outcome for a board task. Authoritative source for \"is it running?\", \"when does it next fire?\", \"did the last run succeed?\" — never infer task state from filesystem listings. For recurring tasks, returns the schedule's next_run_at and last_fired_at.",
            InternalToolType::GetProjectTask,
            get_project_task_schema(),
        ),
    ]
}

pub fn get_project_task_schema() -> Vec<SchemaField> {
    vec![SchemaField {
        name: "task_key".to_string(),
        field_type: "STRING".to_string(),
        description: Some("Existing task key, e.g. `TASK-123456789`.".to_string()),
        required: true,
        ..Default::default()
    }]
}

pub fn delegate_task_schema() -> Vec<SchemaField> {
    vec![
        SchemaField { name: "target_lane_thread_id".to_string(), field_type: "STRING".to_string(), description: Some("Thread ID of an existing EXECUTION lane to receive the task (from list_threads or create_thread).".to_string()), required: true, ..Default::default() },
        SchemaField { name: "title".to_string(), field_type: "STRING".to_string(), description: Some("Short, specific task title (one line).".to_string()), required: true, ..Default::default() },
        SchemaField { name: "description".to_string(), field_type: "STRING".to_string(), description: Some("Task brief: scope boundaries, what to inspect, what to ignore, and the exact deliverable path/name to write under `/delegated_workspace/`.".to_string()), required: true, ..Default::default() },
        SchemaField { name: "capability_tags".to_string(), field_type: "ARRAY".to_string(), items_type: Some("STRING".to_string()), description: Some("Optional matching hints (stable role labels like `research`, `review`, `analysis`).".to_string()), min_items: Some(1), required: false, ..Default::default() },
        SchemaField {
            name: "input_mounts".to_string(),
            field_type: "ARRAY".to_string(),
            description: Some("Optional read-only input folders from this workspace. Each `{ path, alias }`: `path` is an explicit subfolder under `/workspace/` (not `/workspace` itself), exposed to the lane at `/delegated_inputs/<alias>/`. Use when the lane must analyze a folder without copying it.".to_string()),
            max_items: Some(8),
            items_schema: Some(Box::new(SchemaField {
                field_type: "OBJECT".to_string(),
                properties: Some(vec![
                    SchemaField {
                        name: "path".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Existing folder under `/workspace/` (e.g. `/workspace/research`). Not `/workspace` itself.".to_string()),
                        required: true,
                        ..Default::default()
                    },
                    SchemaField {
                        name: "alias".to_string(),
                        field_type: "STRING".to_string(),
                        description: Some("Short mount name; the lane sees the folder at `/delegated_inputs/<alias>/`.".to_string()),
                        required: true,
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            })),
            required: false,
            ..Default::default()
        },
    ]
}

pub fn subscribe_to_task_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "task_key".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Existing task key, e.g. `TASK-123456789`.".to_string()),
            required: true,
            ..Default::default()
        },
        SchemaField {
            name: "event_kinds".to_string(),
            field_type: "ARRAY".to_string(),
            items_type: Some("STRING".to_string()),
            items_schema: Some(Box::new(SchemaField {
                name: "event_kind".to_string(),
                field_type: "STRING".to_string(),
                enum_values: string_enum(&["completed", "blocked", "cancelled"]),
                ..Default::default()
            })),
            description: Some(
                "Optional subset of `completed`, `blocked`, `cancelled`. Defaults to all three."
                    .to_string(),
            ),
            required: false,
            ..Default::default()
        },
    ]
}

pub fn unsubscribe_from_task_schema() -> Vec<SchemaField> {
    vec![SchemaField {
        name: "task_key".to_string(),
        field_type: "STRING".to_string(),
        description: Some("Existing task key the thread is subscribed to.".to_string()),
        required: true,
        ..Default::default()
    }]
}

pub fn list_threads_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "include_conversation_threads".to_string(),
            field_type: "BOOLEAN".to_string(),
            description: Some(
                "Include conversation/user-facing threads. Default false.".to_string(),
            ),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "include_archived".to_string(),
            field_type: "BOOLEAN".to_string(),
            description: Some("Include archived threads. Default false.".to_string()),
            required: false,
            ..Default::default()
        },
    ]
}

pub fn create_project_task_schema() -> Vec<SchemaField> {
    vec![
    SchemaField { name: "title".to_string(), field_type: "STRING".to_string(), description: Some("Short task title.".to_string()), required: true, ..Default::default() },
    SchemaField { name: "description".to_string(), field_type: "STRING".to_string(), description: Some("Optional canonical task description stored at creation.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "status".to_string(), field_type: "STRING".to_string(), description: Some("Optional initial status. Default pending.".to_string()), enum_values: string_enum(&["pending", "in_progress", "blocked", "completed", "failed"]), required: false, ..Default::default() },
    SchemaField { name: "priority".to_string(), field_type: "STRING".to_string(), description: Some("Optional priority. Default neutral.".to_string()), enum_values: string_enum(&["urgent", "high", "neutral", "low"]), required: false, ..Default::default() },
    SchemaField { name: "parent_task_key".to_string(), field_type: "STRING".to_string(), description: Some("Optional existing task key to link this task as a child (`child_of`).".to_string()), required: false, ..Default::default() },
    SchemaField { name: "schedule".to_string(), field_type: "OBJECT".to_string(), description: Some("Optional schedule for a template task. `kind=once` with next_run_at, or `kind=interval` with next_run_at + interval_seconds.".to_string()), required: false, properties: Some(vec![
        SchemaField { name: "kind".to_string(), field_type: "STRING".to_string(), description: Some("`once` or `interval`.".to_string()), enum_values: string_enum(&["once", "interval"]), required: true, ..Default::default() },
        SchemaField { name: "next_run_at".to_string(), field_type: "STRING".to_string(), description: Some("UTC RFC3339 timestamp for the next run.".to_string()), required: true, ..Default::default() },
        SchemaField { name: "interval_seconds".to_string(), field_type: "INTEGER".to_string(), description: Some("Required only for `interval`.".to_string()), minimum: Some(1.0), required: false, ..Default::default() },
    ]), ..Default::default() },
    SchemaField { name: "auto_subscribe".to_string(), field_type: "BOOLEAN".to_string(), description: Some("From a conversation thread, auto-subscribe to the task's completed/blocked/cancelled events. Default true; pass false to dispatch without tracking the outcome.".to_string()), required: false, ..Default::default() },
]
}

pub fn update_project_task_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "task_key".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Existing board task key, e.g. `TASK-123456789`.".to_string()),
            required: true,
            ..Default::default()
        },
        SchemaField {
            name: "title".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Optional updated title (conversation threads may revise it). Editing preempts running execution and re-routes the coordinator.".to_string()),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "description".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Optional updated brief the worker reads (conversation threads may revise it). Direct, sequenced instruction (\"First X. Then Y.\"), not commentary. Editing preempts running execution and re-routes the coordinator.".to_string()),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "status".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Optional updated status. Omit to leave unchanged. `completed` is a strict gate: it requires a substantive `result_summary` AND at least one `artifacts` entry whose `path` is a real deliverable file that exists now — the runtime rejects completion otherwise. Before marking `completed`, cite the executor's deliverable path(s) (from their handoff / `/task/JOURNAL.md`); if no deliverable exists, the task is not done — reroute or block, do not invent a path.".to_string()),
            enum_values: string_enum(&[
                "pending",
                "in_progress",
                "blocked",
                "completed",
                "failed",
                "cancelled",
                "waiting_for_children",
            ]),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "priority".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Optional updated priority. Omit to leave unchanged.".to_string()),
            enum_values: string_enum(&["urgent", "high", "neutral", "low"]),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "schedule".to_string(),
            field_type: "OBJECT".to_string(),
            description: Some("Optional schedule create/replace. `kind=once` with next_run_at, or `kind=interval` with next_run_at + interval_seconds.".to_string()),
            required: false,
            properties: Some(vec![
                SchemaField { name: "kind".to_string(), field_type: "STRING".to_string(), description: Some("`once` or `interval`.".to_string()), enum_values: string_enum(&["once", "interval"]), required: true, ..Default::default() },
                SchemaField { name: "next_run_at".to_string(), field_type: "STRING".to_string(), description: Some("UTC RFC3339 timestamp for the next run.".to_string()), required: true, ..Default::default() },
                SchemaField { name: "interval_seconds".to_string(), field_type: "INTEGER".to_string(), description: Some("Required only for `interval`.".to_string()), minimum: Some(1.0), required: false, ..Default::default() },
            ]),
            ..Default::default()
        },
        SchemaField {
            name: "result_summary".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Required for status completed/failed/blocked/rejected/needs_clarification. Min 30 chars. What was produced (completed) or why it's in this state, so the next reader skips the journal.".to_string()),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "artifacts".to_string(),
            field_type: "ARRAY".to_string(),
            items_schema: Some(Box::new(SchemaField {
                field_type: "OBJECT".to_string(),
                properties: Some(vec![
                    SchemaField { name: "path".to_string(), field_type: "STRING".to_string(), description: Some("Path to the deliverable file in the sandbox (typically `/task/artifacts/...`). Must exist now — missing paths are rejected.".to_string()), required: true, ..Default::default() },
                    SchemaField { name: "kind".to_string(), field_type: "STRING".to_string(), description: Some("Short type label, e.g. `report`, `dataset`, `code`, `image`.".to_string()), required: false, ..Default::default() },
                    SchemaField { name: "note".to_string(), field_type: "STRING".to_string(), description: Some("One-line description of what this deliverable is.".to_string()), required: false, ..Default::default() },
                ]),
                ..Default::default()
            })),
            description: Some("MANDATORY when status is `completed`: at least one entry, each `{path, kind?, note?}`, every `path` a real deliverable file that exists now (executor output, typically under `/task/artifacts/`). Pull the paths from the assignee's handoff / `/task/JOURNAL.md`. Empty list or a missing path is rejected. Omit for non-completion updates.".to_string()),
            min_items: Some(1),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "findings".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Optional one-line handoff: durable facts the next agent needs (e.g. `Webhook secret rotated 2026-05-10; staging not updated`). Max ~200 chars; semicolon-separate multiples. Long context goes in `/task/artifacts/`. Lands in the completion journal entry.".to_string()),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "cautions".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Optional one-line handoff: gotchas/do-nots/destructive actions to avoid (e.g. `Don't run replay.sh — resigns with wrong secret`). Max ~200 chars. Lands in the journal.".to_string()),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "next".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Optional one-line recommendation for the coordinator/next agent (e.g. `assign ops-lane to rotate STAGING_WEBHOOK_SECRET`). Max ~200 chars.".to_string()),
            required: false,
            ..Default::default()
        },
    ]
}

pub fn assign_project_task_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "task_key".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Existing board task key, e.g. `TASK-123456789`.".to_string()),
            required: true,
            ..Default::default()
        },
        project_task_assignments_schema(),
    ]
}

pub fn create_thread_schema() -> Vec<SchemaField> {
    vec![
    SchemaField { name: "title".to_string(), field_type: "STRING".to_string(), description: Some("Lane name. Stable and reusable (e.g. 'Marketing Research Lane', 'Review Lane'), not a task-specific sentence.".to_string()), required: true, ..Default::default() },
    SchemaField { name: "assigned_agent_name".to_string(), field_type: "STRING".to_string(), description: Some("REQUIRED, no default. The agent that owns/executes this lane: either the current coordinator (`agent_name`) or one of `available_sub_agents`. Almost always a specialist from `available_sub_agents` whose responsibility matches the lane. Pick the coordinator only for genuinely coordinator-owned work (rare) — picking it for every lane fans all work back into the coordinator, the most common mistake.".to_string()), required: true, ..Default::default() },
    SchemaField { name: "responsibility".to_string(), field_type: "STRING".to_string(), description: Some("Short durable routing label for what the lane owns (e.g. 'marketing research', 'landing page review'). Describes the long-lived responsibility, not the current task.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "system_instructions".to_string(), field_type: "STRING".to_string(), description: Some("Durable cross-task operating instructions for the lane (~120-160 words max): mission, quality bar, evidence standard, output discipline. Not for one-off task instructions, URLs, current entities, quotas, or tool chatter.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "reusable".to_string(), field_type: "BOOLEAN".to_string(), description: Some("Whether the lane persists and is reused across tasks. true for durable service/review lanes; false only for exceptional temporary lanes. Default true for non-conversation threads.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "accepts_assignments".to_string(), field_type: "BOOLEAN".to_string(), description: Some("Whether the lane may be targeted by project-task assignments. false only when the lane should exist but not be directly assigned. Default true.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "capability_tags".to_string(), field_type: "ARRAY".to_string(), items_type: Some("STRING".to_string()), description: Some("Optional stable matching hints to find this lane later (e.g. `research`, `review`, `approval`). Enduring capabilities, not task details.".to_string()), min_items: Some(1), required: false, ..Default::default() },
    SchemaField { name: "metadata".to_string(), field_type: "OBJECT".to_string(), description: Some("Optional structured metadata for bookkeeping. Avoid unless you have a clear routing/integration reason.".to_string()), required: false, ..Default::default() },
]
}

pub fn update_thread_schema() -> Vec<SchemaField> {
    vec![
    SchemaField { name: "thread_id".to_string(), field_type: "STRING".to_string(), description: Some("Existing thread ID to modify. Use when the lane's durable role or instructions are wrong; not for a one-off task brief.".to_string()), required: true, ..Default::default() },
    SchemaField { name: "title".to_string(), field_type: "STRING".to_string(), description: Some("Optional replacement lane title. Keep stable/reusable.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "responsibility".to_string(), field_type: "STRING".to_string(), description: Some("Optional replacement durable routing label.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "system_instructions".to_string(), field_type: "STRING".to_string(), description: Some("Optional replacement durable lane instructions. Concise, reusable across tasks; not a current task brief.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "reusable".to_string(), field_type: "BOOLEAN".to_string(), description: Some("Optional replacement reusable flag (changes whether the lane persists across tasks).".to_string()), required: false, ..Default::default() },
    SchemaField { name: "accepts_assignments".to_string(), field_type: "BOOLEAN".to_string(), description: Some("Optional replacement assignment-targeting flag.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "capability_tags".to_string(), field_type: "ARRAY".to_string(), items_type: Some("STRING".to_string()), description: Some("Optional complete replacement for the lane's capability tags.".to_string()), min_items: Some(1), required: false, ..Default::default() },
    SchemaField { name: "metadata".to_string(), field_type: "OBJECT".to_string(), description: Some("Optional complete replacement for structured metadata.".to_string()), required: false, ..Default::default() },
]
}
