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
        ("task_graph_reset", "Abandon the current task graph and start fresh. Cancels all pending and in-progress nodes; the next task_graph_add_node auto-creates a new graph. Use when the user course-corrects and the existing plan is no longer valid.", InternalToolType::TaskGraphReset, task_graph_reset_schema()),
    ]
}

fn task_graph_reset_schema() -> Vec<SchemaField> {
    vec![SchemaField {
        name: "reason".to_string(),
        field_type: "STRING".to_string(),
        description: Some(
            "Short explanation of why the current plan is being abandoned (e.g. user clarified scope, new constraint).".to_string(),
        ),
        required: true,
        ..Default::default()
    }]
}

fn task_graph_add_node_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "title".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Task node title.".to_string()),
            required: true,
            ..Default::default()
        },
        SchemaField {
            name: "description".to_string(),
            field_type: "STRING".to_string(),
            description: Some("Optional node description.".to_string()),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "max_retries".to_string(),
            field_type: "INTEGER".to_string(),
            description: Some("Maximum retries for this node (default 2).".to_string()),
            minimum: Some(0.0),
            required: false,
            ..Default::default()
        },
        SchemaField {
            name: "input".to_string(),
            field_type: "OBJECT".to_string(),
            description: Some("Optional structured input payload.".to_string()),
            required: false,
            ..Default::default()
        },
    ]
}

fn task_graph_add_dependency_schema() -> Vec<SchemaField> {
    vec![
        SchemaField {
            name: "from_node_id".to_string(),
            field_type: "STRING".to_string(),
            description: Some(
                "Upstream node id returned by an earlier task_graph_add_node call on a previous turn.".to_string(),
            ),
            required: true,
            ..Default::default()
        },
        SchemaField {
            name: "to_node_id".to_string(),
            field_type: "STRING".to_string(),
            description: Some(
                "Downstream node id returned by an earlier task_graph_add_node call on a previous turn.".to_string(),
            ),
            required: true,
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
