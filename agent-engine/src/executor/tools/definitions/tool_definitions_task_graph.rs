use models::{InternalToolType, SchemaField};

pub(crate) fn task_graph_tools() -> Vec<(
    &'static str,
    &'static str,
    InternalToolType,
    Vec<SchemaField>,
)> {
    vec![
        ("task_graph_add_node", "Add a node to the execution task graph.", InternalToolType::TaskGraphAddNode, task_graph_add_node_schema()),
        ("task_graph_add_dependency", "Add a dependency edge between two graph nodes (from_node -> to_node).", InternalToolType::TaskGraphAddDependency, task_graph_add_dependency_schema()),
        ("task_graph_mark_in_progress", "Mark a graph node in progress when you begin actively working on it.", InternalToolType::TaskGraphMarkInProgress, task_graph_mark_in_progress_schema()),
        ("task_graph_complete_node", "Mark a graph node completed.", InternalToolType::TaskGraphCompleteNode, task_graph_complete_node_schema()),
        ("task_graph_fail_node", "Mark a graph node failed (auto-retries if budget remains).", InternalToolType::TaskGraphFailNode, task_graph_fail_node_schema()),
        ("task_graph_mark_completed", "Mark the graph completed after writing a required next-step handoff file under /task/handoffs/.", InternalToolType::TaskGraphMarkCompleted, task_graph_mark_completed_schema()),
        ("task_graph_mark_failed", "Mark the graph failed when work cannot proceed after retries. Requires a /task/handoffs/ file that explains the failure and the next recovery step.", InternalToolType::TaskGraphMarkFailed, task_graph_mark_failed_schema()),
    ]
}

fn task_graph_add_node_schema() -> Vec<SchemaField> {
    vec![
    SchemaField { name: "node_ref".to_string(), field_type: "STRING".to_string(), description: Some("Optional local reference name for this node so later task-graph calls in the same batch can refer to it with `node_ref`, `from_node_ref`, or `to_node_ref`.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "title".to_string(), field_type: "STRING".to_string(), description: Some("Task node title.".to_string()), required: true, ..Default::default() },
    SchemaField { name: "description".to_string(), field_type: "STRING".to_string(), description: Some("Optional node description.".to_string()), required: false, ..Default::default() },
    SchemaField { name: "max_retries".to_string(), field_type: "INTEGER".to_string(), description: Some("Maximum retries for this node (default 2).".to_string()), minimum: Some(0.0), required: false, ..Default::default() },
    SchemaField { name: "input".to_string(), field_type: "OBJECT".to_string(), description: Some("Optional structured input payload.".to_string()), required: false, ..Default::default() },
]
}

fn task_graph_add_dependency_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "from_node_id".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Existing upstream node id.".to_string()),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "from_node_ref".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Optional local batch reference for the upstream node.".to_string()),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "to_node_id".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Existing downstream node id.".to_string()),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "to_node_ref".to_string(),
            field_type: "STRING".to_string(),
            description: Some(
                "Optional local batch reference for the downstream node.".to_string(),
            ),
            required: false,
            ..Default::default()
        },
    ]
}

fn task_graph_mark_in_progress_schema() -> Vec<SchemaField> {
    vec![SchemaField {
        name: "node_id".to_string(),
        field_type: "STRING".to_string(),
        description: Some("Node id to mark in progress.".to_string()),
        required: true,
        ..Default::default()
    }]
}

fn task_graph_complete_node_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "node_id".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Node id to mark completed.".to_string()),
            required: true,
            ..Default::default()
        },
        SchemaField {
            name: "output".to_string(),
            field_type: "OBJECT".to_string(),
            description: Some("Optional structured completion output.".to_string()),
            required: false,
            ..Default::default()
        },
    ]
}

fn task_graph_fail_node_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "node_id".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Node id to mark failed.".to_string()),
            required: true,
            ..Default::default()
        },
        SchemaField {
            name: "reason".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Failure reason.".to_string()),
            required: false,
            ..Default::default()
        },
    ]
}

fn task_graph_mark_completed_schema() -> Vec<SchemaField> {
    vec![SchemaField {
        name: "handoff_file_path".to_string(),
        field_type: "STRING".to_string(),
        description: Some(
            "Required /task/handoffs/ path that captures the next-step summary.".to_string(),
        ),
        required: true,
        ..Default::default()
    }]
}

fn task_graph_mark_failed_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "handoff_file_path".to_string(),
            field_type: "STRING".to_string(),
            description: Some(
                "Required /task/handoffs/ path that explains the failure and next recovery step."
                    .to_string(),
            ),
            required: true,
            ..Default::default()
        },
        SchemaField {
            name: "reason".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Optional short failure reason.".to_string()),
            required: false,
            ..Default::default()
        },
    ]
}
