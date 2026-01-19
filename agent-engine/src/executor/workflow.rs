use super::core::AgentExecutor;

use common::error::AppError;
use dto::json::{StreamEvent, WorkflowCall, WorkflowExecutionResult};
use models::{AiWorkflow, WorkflowEdge, WorkflowNode, WorkflowNodeType};
use serde_json::{json, Value};
use std::collections::HashMap;

impl AgentExecutor {
    pub async fn execute_workflow_task(
        &self,
        workflow_call: &WorkflowCall,
        workflows: &[AiWorkflow],
        conversation_context: &[Value],
        memory_context: &[Value],
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        let workflow = workflows
            .iter()
            .find(|w| w.name == workflow_call.workflow_name)
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Workflow {} not found",
                    workflow_call.workflow_name
                ))
            })?;

        let mut initial_state = workflow_call.inputs.clone();
        if let Some(obj) = initial_state.as_object_mut() {
            obj.insert(
                "conversation_history".to_string(),
                json!(conversation_context),
            );
            obj.insert("memory_context".to_string(), json!(memory_context));
            obj.insert(
                "total_context_items".to_string(),
                json!(conversation_context.len() + memory_context.len()),
            );
        }

        self.execute_workflow(workflow, initial_state, channel)
            .await
    }

    pub async fn execute_workflow(
        &self,
        workflow: &AiWorkflow,
        input_data: Value,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        let trigger_node = workflow
            .workflow_definition
            .nodes
            .iter()
            .find(|node| matches!(node.node_type, WorkflowNodeType::Trigger(_)))
            .ok_or_else(|| AppError::BadRequest("No trigger node found in workflow".to_string()))?;

        let mut workflow_state = HashMap::new();
        if let Some(input_obj) = input_data.as_object() {
            for (key, value) in input_obj {
                workflow_state.insert(key.clone(), value.clone());
            }
        }

        let output = self
            .execute_node_recursive(
                trigger_node,
                &workflow.workflow_definition.nodes,
                &workflow.workflow_definition.edges,
                workflow_state,
                channel.clone(),
                0,
            )
            .await?;

        if let Some(status) = output.get("status").and_then(|s| s.as_str()) {
            if status == "pending" {
                let result = WorkflowExecutionResult {
                    workflow_id: workflow.id,
                    workflow_name: workflow.name.clone(),
                    execution_status: "pending".to_string(),
                    output: Some(output),
                    message: None,
                };
                return Ok(serde_json::to_value(&result)?);
            }
        }

        let result = WorkflowExecutionResult {
            workflow_id: workflow.id,
            workflow_name: workflow.name.clone(),
            execution_status: "completed".to_string(),
            output: Some(output),
            message: None,
        };
        Ok(serde_json::to_value(&result)?)
    }

    pub(super) fn execute_node_recursive<'a>(
        &'a self,
        node: &'a WorkflowNode,
        all_nodes: &'a [WorkflowNode],
        all_edges: &'a [WorkflowEdge],
        mut workflow_state: HashMap<String, Value>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
        depth: usize,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, AppError>> + Send + 'a>>
    {
        Box::pin(async move {
            if depth > 100 {
                return Err(AppError::Internal(
                    "Workflow execution depth limit exceeded".to_string(),
                ));
            }

            let result = self
                .execute_node(
                    &node.node_type,
                    all_nodes,
                    all_edges,
                    &mut workflow_state,
                    channel.clone(),
                    depth,
                )
                .await;

            let output = result?;

            if let Some(status) = output.get("status").and_then(|s| s.as_str()) {
                if status == "pending" {
                    return Ok(output);
                }
            }

            workflow_state.insert(format!("{}_output", node.id), output.clone());

            let next_edges: Vec<&WorkflowEdge> = all_edges
                .iter()
                .filter(|edge| edge.source == node.id)
                .collect();

            self.process_next_nodes(
                node,
                all_nodes,
                all_edges,
                workflow_state,
                channel,
                depth,
                output,
                next_edges,
            )
            .await
        })
    }

    pub(super) async fn process_next_nodes(
        &self,
        node: &WorkflowNode,
        all_nodes: &[WorkflowNode],
        all_edges: &[WorkflowEdge],
        workflow_state: HashMap<String, Value>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
        depth: usize,
        output: Value,
        next_edges: Vec<&WorkflowEdge>,
    ) -> Result<Value, AppError> {
        if node.node_type.type_name() == "Switch" {
            return self
                .process_switch_node_edges(
                    all_nodes,
                    all_edges,
                    workflow_state,
                    channel,
                    depth,
                    output,
                    next_edges,
                )
                .await;
        }

        match next_edges.len() {
            0 => Ok(output),
            1 => {
                self.process_single_edge(
                    all_nodes,
                    all_edges,
                    workflow_state,
                    channel,
                    depth,
                    output,
                    next_edges[0],
                )
                .await
            }
            _ => Err(AppError::BadRequest(format!(
                "Node {} has multiple outgoing edges but is not a switch node",
                node.id
            ))),
        }
    }

    async fn process_switch_node_edges(
        &self,
        all_nodes: &[WorkflowNode],
        all_edges: &[WorkflowEdge],
        workflow_state: HashMap<String, Value>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
        depth: usize,
        output: Value,
        next_edges: Vec<&WorkflowEdge>,
    ) -> Result<Value, AppError> {
        let matched_case = match output.get("matched_case") {
            Some(case) => case,
            None => return Ok(output),
        };

        let case_handle = if matched_case == "default" {
            "default".to_string()
        } else if let Some(case_idx) = matched_case.as_u64() {
            format!("case-{case_idx}")
        } else {
            return Ok(output);
        };

        let matching_edge = next_edges
            .into_iter()
            .find(|e| e.source_handle.as_ref() == Some(&case_handle));

        match matching_edge {
            Some(edge) => self
                .execute_edge(all_nodes, all_edges, workflow_state, channel, depth, edge)
                .await
                .or(Ok(output)),
            None => Ok(output),
        }
    }

    async fn process_single_edge(
        &self,
        all_nodes: &[WorkflowNode],
        all_edges: &[WorkflowEdge],
        workflow_state: HashMap<String, Value>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
        depth: usize,
        output: Value,
        edge: &WorkflowEdge,
    ) -> Result<Value, AppError> {
        self.execute_edge(all_nodes, all_edges, workflow_state, channel, depth, edge)
            .await
            .or(Ok(output))
    }

    async fn execute_edge(
        &self,
        all_nodes: &[WorkflowNode],
        all_edges: &[WorkflowEdge],
        workflow_state: HashMap<String, Value>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
        depth: usize,
        edge: &WorkflowEdge,
    ) -> Result<Value, AppError> {
        let next_node = all_nodes
            .iter()
            .find(|n| n.id == edge.target)
            .ok_or_else(|| AppError::Internal(format!("Target node {} not found", edge.target)))?;

        self.execute_node_recursive(
            next_node,
            all_nodes,
            all_edges,
            workflow_state,
            channel,
            depth + 1,
        )
        .await
    }

    pub async fn resume_workflow_execution(&mut self) -> Result<Value, AppError> {
        let workflow_id = self.current_workflow_id.ok_or_else(|| {
            AppError::Internal("No workflow ID found in resume state".to_string())
        })?;

        let ctx = self.ctx.clone();
        let workflow = ctx
            .agent
            .workflows
            .iter()
            .find(|w| w.id == workflow_id)
            .ok_or_else(|| AppError::Internal(format!("Workflow {} not found", workflow_id)))?;

        let current_node_id = self.current_workflow_node_id.as_ref().ok_or_else(|| {
            AppError::Internal("No current node ID found in resume state".to_string())
        })?;

        let current_node = workflow
            .workflow_definition
            .nodes
            .iter()
            .find(|n| &n.id == current_node_id)
            .ok_or_else(|| {
                AppError::Internal(format!("Node {} not found in workflow", current_node_id))
            })?;

        let workflow_state = self.current_workflow_state.clone().ok_or_else(|| {
            AppError::Internal("No workflow state found in resume state".to_string())
        })?;

        let node_output_key = format!("{}_output", current_node_id);
        if let Some(pending_output) = workflow_state.get(&node_output_key) {
            if let Some(_) = pending_output
                .get("execution_id")
                .and_then(|id| id.as_u64())
            {
                if let Some(node_output) = workflow_state.get(&node_output_key) {
                    if !node_output
                        .get("status")
                        .and_then(|s| s.as_str())
                        .map_or(false, |s| s == "pending")
                    {
                        self.current_workflow_state = Some(workflow_state.clone());

                        let next_edges: Vec<&WorkflowEdge> = workflow
                            .workflow_definition
                            .edges
                            .iter()
                            .filter(|edge| edge.source == *current_node_id)
                            .collect();

                        return self
                            .process_next_nodes(
                                current_node,
                                &workflow.workflow_definition.nodes,
                                &workflow.workflow_definition.edges,
                                workflow_state.clone(),
                                self.channel.clone(),
                                self.current_workflow_execution_path.len(),
                                node_output.clone(),
                                next_edges,
                            )
                            .await;
                    }
                }
            }
        }

        Ok(json!({
            "workflow_id": workflow.id,
            "workflow_name": workflow.name,
            "execution_status": "pending",
            "message": "Waiting for platform function result"
        }))
    }
}
