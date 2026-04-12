use super::core::AgentExecutor;
use super::tool_params::PlannedToolCall;

use common::error::AppError;
use dto::json::agent_executor::{
    TaskGraphAddDependencyParams, TaskGraphCompleteNodeParams, TaskGraphFailNodeParams,
    TaskGraphNodeTargetParams, ToolCallRequest,
};
use serde_json::Value;
use std::collections::HashMap;
impl AgentExecutor {
    pub(crate) fn resolve_task_graph_target(
        params: &TaskGraphNodeTargetParams,
        task_graph_node_refs: &HashMap<String, i64>,
    ) -> Result<TaskGraphNodeTargetParams, AppError> {
        if params.node_id.is_some() {
            return Ok(TaskGraphNodeTargetParams {
                node_id: params.node_id,
                node_ref: None,
            });
        }

        let Some(node_ref) = params.node_ref.as_deref() else {
            return Err(AppError::BadRequest(
                "Task-graph action requires either `node_id` or `node_ref`".to_string(),
            ));
        };

        let Some(node_id) = task_graph_node_refs.get(node_ref) else {
            return Err(AppError::BadRequest(format!(
                "Task-graph reference '{}' has not been created yet in this handoff",
                node_ref
            )));
        };

        Ok(TaskGraphNodeTargetParams {
            node_id: Some((*node_id).into()),
            node_ref: None,
        })
    }

    pub(crate) fn resolve_task_graph_dependency_params(
        params: &TaskGraphAddDependencyParams,
        task_graph_node_refs: &HashMap<String, i64>,
    ) -> Result<TaskGraphAddDependencyParams, AppError> {
        let from_node_id = if let Some(node_id) = params.from_node_id {
            Some(node_id)
        } else if let Some(node_ref) = params.from_node_ref.as_deref() {
            let Some(node_id) = task_graph_node_refs.get(node_ref) else {
                return Err(AppError::BadRequest(format!(
                    "Task-graph reference '{}' has not been created yet in this handoff",
                    node_ref
                )));
            };
            Some((*node_id).into())
        } else {
            return Err(AppError::BadRequest(
                "task_graph_add_dependency requires either `from_node_id` or `from_node_ref`"
                    .to_string(),
            ));
        };

        let to_node_id = if let Some(node_id) = params.to_node_id {
            Some(node_id)
        } else if let Some(node_ref) = params.to_node_ref.as_deref() {
            let Some(node_id) = task_graph_node_refs.get(node_ref) else {
                return Err(AppError::BadRequest(format!(
                    "Task-graph reference '{}' has not been created yet in this handoff",
                    node_ref
                )));
            };
            Some((*node_id).into())
        } else {
            return Err(AppError::BadRequest(
                "task_graph_add_dependency requires either `to_node_id` or `to_node_ref`"
                    .to_string(),
            ));
        };

        Ok(TaskGraphAddDependencyParams {
            from_node_id,
            from_node_ref: None,
            to_node_id,
            to_node_ref: None,
        })
    }

    pub(crate) fn resolve_task_graph_request(
        call: &PlannedToolCall,
        task_graph_node_refs: &HashMap<String, i64>,
    ) -> Result<ToolCallRequest, AppError> {
        match &call.request {
            ToolCallRequest::TaskGraphAddDependency { params } => {
                Ok(ToolCallRequest::TaskGraphAddDependency {
                    params: Self::resolve_task_graph_dependency_params(
                        params,
                        task_graph_node_refs,
                    )?,
                })
            }
            ToolCallRequest::TaskGraphMarkInProgress { params } => {
                Ok(ToolCallRequest::TaskGraphMarkInProgress {
                    params: Self::resolve_task_graph_target(params, task_graph_node_refs)?,
                })
            }
            ToolCallRequest::TaskGraphCompleteNode { params } => {
                Ok(ToolCallRequest::TaskGraphCompleteNode {
                    params: TaskGraphCompleteNodeParams {
                        target: Self::resolve_task_graph_target(
                            &params.target,
                            task_graph_node_refs,
                        )?,
                        output: params.output.clone(),
                    },
                })
            }
            ToolCallRequest::TaskGraphFailNode { params } => {
                Ok(ToolCallRequest::TaskGraphFailNode {
                    params: TaskGraphFailNodeParams {
                        target: Self::resolve_task_graph_target(
                            &params.target,
                            task_graph_node_refs,
                        )?,
                        error: params.error.clone(),
                    },
                })
            }
            _ => Ok(call.request.clone()),
        }
    }

    pub(crate) fn capture_task_graph_node_ref(
        call: &PlannedToolCall,
        result_value: &Value,
        task_graph_node_refs: &mut HashMap<String, i64>,
    ) {
        let ToolCallRequest::TaskGraphAddNode { params, .. } = &call.request else {
            return;
        };
        let Some(node_ref) = params.node_ref.as_deref() else {
            return;
        };

        let Some(node_id_str) = result_value
            .get("node")
            .and_then(|node| node.get("id"))
            .and_then(|value| value.as_str())
        else {
            return;
        };

        if let Ok(node_id) = node_id_str.parse::<i64>() {
            task_graph_node_refs.insert(node_ref.to_string(), node_id);
        }
    }
}
