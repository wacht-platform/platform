//! Tool definitions for the agent executor.

use models::{InternalToolType, SchemaField};

pub fn internal_tools() -> Vec<(
    &'static str,
    &'static str,
    InternalToolType,
    Vec<SchemaField>,
)> {
    let mut tools = super::tool_definitions_internal::internal_tools();
    tools.extend(super::tool_definitions_project::project_tools());
    tools.extend(super::tool_definitions_task_graph::task_graph_tools());
    tools
}
