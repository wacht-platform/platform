use serde::Deserialize;
use super::stage_execution::{
    ActionExecution, ActionExecutionDetails, ToolCallExecution,
    WorkflowCallExecution, ContextSearchExecution, MemoryOperationExecution
};

/// Wrapper for deserializing ActionExecution from XML
/// This handles the nested structure that quick_xml expects
