use super::tool_definitions_common::string_enum;
use models::{InternalToolType, SchemaField};

fn project_task_assignments_schema() -> SchemaField {
    SchemaField {
        name: "assignments".to_string(),
        field_type: "ARRAY".to_string(),
        description: Some("Ordered assignment chain. Each item may include thread_id or a reusable-thread selector via responsibility/capability_tags, plus assignment_role (`executor`, `reviewer`, `specialist_reviewer`, `approver`, `observer`), assignment_order, status, instructions, and handoff_file_path. Ordering is 1-based within the submitted batch: use 1 for the first assignment in the batch, 2 for the next, and so on. Runtime appends the batch after existing assignment history for the task. When a stage completes successfully, the backend auto-activates the next pending stage if one exists.".to_string()),
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
                    description: Some("Optional durable lane responsibility selector when assigning by lane role instead of explicit thread ID.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "capability_tags".to_string(),
                    field_type: "ARRAY".to_string(),
                    items_type: Some("STRING".to_string()),
                    description: Some("Optional durable lane capability tags used for assignment matching.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "assignment_role".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Assignment stage role: executor, reviewer, specialist_reviewer, approver, or observer.".to_string()),
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
                    name: "assignment_order".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Optional explicit 1-based stage order for the assignment chain. Use 1 for the first active stage, 2 for the next stage, and so on. Omit when simple list order is already correct.".to_string()),
                    minimum: Some(1.0),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "status".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional initial assignment status. Usually omit this and let runtime defaults apply: stage 1 defaults to `available`, later stages default to `pending`. Only set it explicitly when overriding normal staged routing.".to_string()),
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
                    description: Some("Optional task-specific instructions for this assignment stage.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "handoff_file_path".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional /task/handoffs/ file path containing the richer brief for this assignment stage.".to_string()),
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
            "Create a new task in the shared project task board. Available to the coordinator and user-facing conversation threads. The runtime generates a fresh task key automatically. When a user-facing conversation thread creates a task, it is routed to the coordinator automatically. Use this when the user wants durable delegated work, including requests phrased as background work, async follow-up, or separate work that should continue while the current thread stays focused. Optionally attach it as a child of an existing task by `parent_task_key`.",
            InternalToolType::CreateProjectTask,
            create_project_task_schema(),
        ),
        (
            "update_project_task",
            "Update an existing shared project task by task key. Use it for status, priority, outputs, and blockers only. Omit unchanged fields.",
            InternalToolType::UpdateProjectTask,
            update_project_task_schema(),
        ),
        (
            "assign_project_task",
            "Replace the current assignment plan for an existing shared project task. Coordinator-only. Use it to route work through execution and review lanes by task key.",
            InternalToolType::AssignProjectTask,
            assign_project_task_schema(),
        ),
        (
            "list_threads",
            "List threads in the current project so work can be assigned through the task board against real thread lanes.",
            InternalToolType::ListThreads,
            list_threads_schema(),
        ),
        (
            "create_thread",
            "Create a durable execution lane in the current project. Coordinator-only. Delegation still happens through project-task assignments.",
            InternalToolType::CreateThread,
            create_thread_schema(),
        ),
        (
            "update_thread",
            "Update an existing durable thread/lane in the current project. Coordinator-only. Use this to change title, responsibility, instructions, or assignment capability before routing work through the task board.",
            InternalToolType::UpdateThread,
            update_thread_schema(),
        ),
    ]
}

pub fn list_threads_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "include_conversation_threads".to_string(),
            field_type: "BOOLEAN".to_string(),
            description: Some(
                "Include conversation/user-facing threads in the results. Default: false."
                    .to_string(),
            ),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "include_archived".to_string(),
            field_type: "BOOLEAN".to_string(),
            description: Some(
                "Include archived threads in the results. Default: false.".to_string(),
            ),
            required: false,
            ..Default::default()
        },
    ]
}

pub fn create_project_task_schema() -> Vec<SchemaField> {
    vec![
    SchemaField { name: "title".to_string(), field_type: "STRING".to_string(), description: Some("Short task title.".to_string()), required: true, ..Default::default() },
    SchemaField { name: "description".to_string(), field_type: "STRING".to_string(), description: Some("Optional canonical task description to store at creation time.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "status".to_string(), field_type: "STRING".to_string(), description: Some("Optional initial task status (`pending`, `in_progress`, `blocked`, `completed`, `failed`). Default: pending.".to_string()), enum_values: string_enum(&["pending", "in_progress", "blocked", "completed", "failed"]), required: false, ..Default::default() },
    SchemaField { name: "priority".to_string(), field_type: "STRING".to_string(), description: Some("Optional priority (`urgent`, `high`, `neutral`, `low`). Default: neutral.".to_string()), enum_values: string_enum(&["urgent", "high", "neutral", "low"]), required: false, ..Default::default() },
    SchemaField { name: "parent_task_key".to_string(), field_type: "STRING".to_string(), description: Some("Optional existing task key to link this new task as a child task (`child_of`) under that parent.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "schedule".to_string(), field_type: "OBJECT".to_string(), description: Some("Optional schedule for a template task. Use `kind = once` with `next_run_at`, or `kind = interval` with `next_run_at` and `interval_seconds`.".to_string()), required: false, properties: Some(vec![
        SchemaField { name: "kind".to_string(), field_type: "STRING".to_string(), description: Some("Schedule kind: `once` or `interval`.".to_string()), enum_values: string_enum(&["once", "interval"]), required: true, ..Default::default() },
        SchemaField { name: "next_run_at".to_string(), field_type: "STRING".to_string(), description: Some("UTC RFC3339 timestamp for the next run.".to_string()), required: true, ..Default::default() },
        SchemaField { name: "interval_seconds".to_string(), field_type: "INTEGER".to_string(), description: Some("Required only for `interval` schedules.".to_string()), minimum: Some(1.0), required: false, ..Default::default() },
    ]), ..Default::default() },
]
}

pub fn update_project_task_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "task_key".to_string(),
            field_type: "STRING".to_string(),
            description: Some(
                "Existing task key in the shared project task board, for example `TASK-123456789`."
                    .to_string(),
            ),
            required: true,
            ..Default::default()
        },
        SchemaField {
            name: "status".to_string(),
            field_type: "STRING".to_string(),
            description: Some(
                "Optional updated task status. Omit when status should stay unchanged.".to_string(),
            ),
            enum_values: string_enum(&[
                "pending",
                "in_progress",
                "blocked",
                "completed",
                "failed",
                "waiting_for_children",
            ]),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "priority".to_string(),
            field_type: "STRING".to_string(),
            description: Some(
                "Optional updated priority. Omit when priority should stay unchanged.".to_string(),
            ),
            enum_values: string_enum(&["urgent", "high", "neutral", "low"]),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "schedule".to_string(),
            field_type: "OBJECT".to_string(),
            description: Some("Optional schedule create/replace payload. Use `kind = once` with `next_run_at`, or `kind = interval` with `next_run_at` and `interval_seconds`.".to_string()),
            required: false,
            properties: Some(vec![
                SchemaField { name: "kind".to_string(), field_type: "STRING".to_string(), description: Some("Schedule kind: `once` or `interval`.".to_string()), enum_values: string_enum(&["once", "interval"]), required: true, ..Default::default() },
                SchemaField { name: "next_run_at".to_string(), field_type: "STRING".to_string(), description: Some("UTC RFC3339 timestamp for the next run.".to_string()), required: true, ..Default::default() },
                SchemaField { name: "interval_seconds".to_string(), field_type: "INTEGER".to_string(), description: Some("Required only for `interval` schedules.".to_string()), minimum: Some(1.0), required: false, ..Default::default() },
            ]),
            ..Default::default()
        },
    ]
}

pub fn assign_project_task_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "task_key".to_string(),
            field_type: "STRING".to_string(),
            description: Some(
                "Existing task key in the shared project task board, for example `TASK-123456789`."
                    .to_string(),
            ),
            required: true,
            ..Default::default()
        },
        project_task_assignments_schema(),
    ]
}

pub fn create_thread_schema() -> Vec<SchemaField> {
    vec![
    SchemaField { name: "title".to_string(), field_type: "STRING".to_string(), description: Some("Human-readable lane name. Use a stable, reusable title such as 'Marketing Research Lane' or 'Review Lane', not a task-specific sentence.".to_string()), required: true, ..Default::default() },
    SchemaField { name: "assigned_agent_name".to_string(), field_type: "STRING".to_string(), description: Some("Optional agent to bind this lane to. Must be either the current coordinator agent (`agent_name`) or one of the listed `available_sub_agents`. Defaults to the current coordinator agent.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "responsibility".to_string(), field_type: "STRING".to_string(), description: Some("Short durable routing label for what this lane owns, such as 'marketing research', 'landing page review', or 'approval'. This is used for assignment targeting and should describe the lane's long-lived responsibility, not the current task.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "system_instructions".to_string(), field_type: "STRING".to_string(), description: Some("Durable operating instructions for how this lane should behave across many tasks. Keep this concise (about 120-160 words max). Use it for lane mission, quality bar, evidence standard, and output discipline. Do not use it for one-off task instructions, URLs, current-task entities, deliverable quotas, or tool-call chatter.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "reusable".to_string(), field_type: "BOOLEAN".to_string(), description: Some("Whether this lane should stay around and be reused across many project tasks. Use `true` for durable service/review lanes. Use `false` only for exceptional task-specific or temporary lanes. Default: true for non-conversation threads.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "accepts_assignments".to_string(), field_type: "BOOLEAN".to_string(), description: Some("Whether this execution lane may be targeted by project-task assignments. Set `true` for lanes that should receive delegated work. Set `false` only when the lane should exist but not be directly assigned. Default: true.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "capability_tags".to_string(), field_type: "ARRAY".to_string(), items_type: Some("STRING".to_string()), description: Some("Optional stable matching hints used to find this lane later, such as `research`, `marketing`, `review`, or `approval`. These should describe enduring capabilities, not one-off task details.".to_string()), min_items: Some(1), required: false, ..Default::default() },
    SchemaField { name: "metadata".to_string(), field_type: "OBJECT".to_string(), description: Some("Optional structured metadata for system bookkeeping. Avoid this unless you have a clear routing or integration reason.".to_string()), required: false, ..Default::default() },
]
}

pub fn update_thread_schema() -> Vec<SchemaField> {
    vec![
    SchemaField { name: "thread_id".to_string(), field_type: "STRING".to_string(), description: Some("Existing thread ID to modify. Update a lane when its durable role or instructions are wrong; do not use this to pass a one-off task brief.".to_string()), required: true, ..Default::default() },
    SchemaField { name: "title".to_string(), field_type: "STRING".to_string(), description: Some("Optional replacement lane title. Keep it stable and reusable rather than task-specific.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "responsibility".to_string(), field_type: "STRING".to_string(), description: Some("Optional replacement durable routing label for what this lane owns.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "system_instructions".to_string(), field_type: "STRING".to_string(), description: Some("Optional replacement durable operating instructions for the lane. Keep it concise and reusable across many tasks; do not paste a current task brief into this field.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "reusable".to_string(), field_type: "BOOLEAN".to_string(), description: Some("Optional replacement reusable flag. Use with care because it changes whether the lane should persist across tasks.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "accepts_assignments".to_string(), field_type: "BOOLEAN".to_string(), description: Some("Optional replacement assignment-targeting flag for whether this lane may receive project-task assignments.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "capability_tags".to_string(), field_type: "ARRAY".to_string(), items_type: Some("STRING".to_string()), description: Some("Optional complete replacement for the lane's stable capability-matching tags.".to_string()), min_items: Some(1), required: false, ..Default::default() },
    SchemaField { name: "metadata".to_string(), field_type: "OBJECT".to_string(), description: Some("Optional complete replacement for structured system metadata.".to_string()), required: false, ..Default::default() },
]
}
