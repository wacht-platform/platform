use super::{AgentContext, ToolCall, ToolExecutor};
use chrono::Utc;
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;
use serde_json::{Value, json};
use shared::error::AppError;
use shared::models::{
    AiWorkflow, ConditionNodeConfig, ErrorHandlerNodeConfig, ExecutionStatus,
    FetchContextNodeConfig, LLMCallNodeConfig, NodeExecution, StoreContextNodeConfig,
    SwitchNodeConfig, ToolCallNodeConfig, WorkflowDefinition, WorkflowNode, WorkflowNodeType,
};
use shared::state::AppState;
use std::collections::{HashMap, HashSet};

pub struct WorkflowEngine {
    pub context: AgentContext,
    pub app_state: AppState,
    pub conversation_history: Vec<ChatMessage>,
}

#[derive(Debug, Clone)]
pub struct WorkflowExecution {
    pub workflow_id: i64,
    pub execution_id: String,
    pub status: ExecutionStatus,
    pub current_node: Option<String>,
    pub execution_context: WorkflowExecutionContext,
    pub node_executions: HashMap<String, NodeExecution>,
    pub completed_at: Option<chrono::DateTime<Utc>>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkflowExecutionContext {
    pub variables: HashMap<String, Value>,
    pub input_data: Value,
    pub output_data: Option<Value>,
    pub memory: HashMap<String, Value>,
    pub tool_results: HashMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct NodeExecutionResult {
    pub status: ExecutionStatus,
    pub output_data: Option<Value>,
    pub error_message: Option<String>,
    pub next_nodes: Vec<String>,
    pub execution_time_ms: u64,
}

impl WorkflowEngine {
    pub fn new(
        context: AgentContext,
        app_state: AppState,
        conversation_history: Vec<ChatMessage>,
    ) -> Self {
        Self {
            context,
            app_state,
            conversation_history,
        }
    }

    pub async fn execute_workflow(
        &self,
        workflow: &AiWorkflow,
        input_data: Value,
        trigger_condition: Option<String>,
    ) -> Result<WorkflowExecution, AppError> {
        let execution_id = (self.app_state.sf.next_id()? as i64).to_string();
        let mut execution = WorkflowExecution {
            workflow_id: workflow.id,
            execution_id: execution_id.clone(),
            status: ExecutionStatus::Running,
            current_node: None,
            execution_context: WorkflowExecutionContext {
                variables: HashMap::new(),
                input_data: input_data.clone(),
                output_data: None,
                memory: HashMap::new(),
                tool_results: HashMap::new(),
            },
            node_executions: HashMap::new(),
            completed_at: None,
            error_message: None,
        };

        // Find trigger nodes
        let trigger_nodes =
            self.find_trigger_nodes(&workflow.workflow_definition, trigger_condition.as_deref())?;

        if trigger_nodes.is_empty() {
            return Err(AppError::BadRequest(
                "No valid trigger nodes found for workflow".to_string(),
            ));
        }

        // Execute workflow starting from trigger nodes
        for trigger_node in trigger_nodes {
            match self
                .execute_node_chain(
                    &workflow.workflow_definition,
                    &trigger_node.id,
                    &mut execution,
                )
                .await
            {
                Ok(_) => {
                    execution.status = ExecutionStatus::Completed;
                    execution.completed_at = Some(Utc::now());
                }
                Err(e) => {
                    execution.status = ExecutionStatus::Failed;
                    execution.error_message = Some(e.to_string());
                    execution.completed_at = Some(Utc::now());
                    return Err(e);
                }
            }
        }

        Ok(execution)
    }

    fn find_trigger_nodes(
        &self,
        definition: &WorkflowDefinition,
        _trigger_condition: Option<&str>,
    ) -> Result<Vec<WorkflowNode>, AppError> {
        let mut trigger_nodes = Vec::new();

        for node in &definition.nodes {
            if let WorkflowNodeType::Trigger(_trigger_config) = &node.node_type {
                trigger_nodes.push(node.clone());
            }
        }

        Ok(trigger_nodes)
    }

    async fn execute_node_chain(
        &self,
        definition: &WorkflowDefinition,
        start_node_id: &str,
        execution: &mut WorkflowExecution,
    ) -> Result<(), AppError> {
        let mut current_nodes = vec![start_node_id.to_string()];
        let mut visited_nodes = HashSet::new();

        while !current_nodes.is_empty() {
            let mut next_nodes = Vec::new();

            for node_id in current_nodes {
                if visited_nodes.contains(&node_id) {
                    continue; // Prevent infinite loops
                }
                visited_nodes.insert(node_id.clone());

                execution.current_node = Some(node_id.clone());

                let node = definition
                    .nodes
                    .iter()
                    .find(|n| n.id == node_id)
                    .ok_or_else(|| AppError::BadRequest(format!("Node '{}' not found", node_id)))?;

                let result = self.execute_single_node(node, execution).await?;

                // Store node execution result
                execution.node_executions.insert(
                    node_id.clone(),
                    NodeExecution {
                        node_id: node_id.clone(),
                        status: result.status.clone(),
                        started_at: Some(Utc::now()),
                        completed_at: Some(Utc::now()),
                        input_data: Some(execution.execution_context.input_data.clone()),
                        output_data: result.output_data.clone(),
                        error_message: result.error_message.clone(),
                        retry_count: 0,
                    },
                );

                if matches!(result.status, ExecutionStatus::Failed) {
                    return Err(AppError::Internal(
                        result
                            .error_message
                            .unwrap_or_else(|| "Node execution failed".to_string()),
                    ));
                }

                // Add next nodes to execution queue
                next_nodes.extend(result.next_nodes);
            }

            current_nodes = next_nodes;
        }

        Ok(())
    }

    async fn execute_single_node(
        &self,
        node: &WorkflowNode,
        execution: &mut WorkflowExecution,
    ) -> Result<NodeExecutionResult, AppError> {
        let start_time = std::time::Instant::now();

        let result = match &node.node_type {
            WorkflowNodeType::Trigger(_) => Ok(NodeExecutionResult {
                status: ExecutionStatus::Completed,
                output_data: Some(execution.execution_context.input_data.clone()),
                error_message: None,
                next_nodes: self
                    .get_next_node_ids(&node.id, &execution.workflow_id)
                    .await?,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
            }),
            WorkflowNodeType::Condition(config) => {
                self.execute_condition_node(node, config, execution).await
            }
            WorkflowNodeType::LLMCall(config) => {
                self.execute_llm_call_node(node, config, execution).await
            }
            WorkflowNodeType::ToolCall(config) => {
                self.execute_tool_call_node(node, config, execution).await
            }
            WorkflowNodeType::Switch(config) => {
                self.execute_switch_node(node, config, execution).await
            }
            WorkflowNodeType::StoreContext(config) => {
                self.execute_store_context_node(node, config, execution)
                    .await
            }
            WorkflowNodeType::FetchContext(config) => {
                self.execute_fetch_context_node(node, config, execution)
                    .await
            }
            WorkflowNodeType::ErrorHandler(config) => {
                self.execute_error_handler_node(node, config, execution)
                    .await
            }
        };

        result.map(|mut r| {
            r.execution_time_ms = start_time.elapsed().as_millis() as u64;
            r
        })
    }

    async fn execute_condition_node(
        &self,
        node: &WorkflowNode,
        config: &ConditionNodeConfig,
        execution: &WorkflowExecution,
    ) -> Result<NodeExecutionResult, AppError> {
        // Evaluate condition using LLM
        let condition_result = self
            .evaluate_condition(&config.expression, execution)
            .await?;

        let next_nodes = if condition_result {
            self.get_next_node_ids(&node.id, &execution.workflow_id)
                .await?
        } else {
            // Find alternative path or end execution
            Vec::new()
        };

        Ok(NodeExecutionResult {
            status: ExecutionStatus::Completed,
            output_data: Some(json!({ "condition_result": condition_result })),
            error_message: None,
            next_nodes,
            execution_time_ms: 0,
        })
    }

    async fn execute_llm_call_node(
        &self,
        node: &WorkflowNode,
        config: &LLMCallNodeConfig,
        execution: &WorkflowExecution,
    ) -> Result<NodeExecutionResult, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.0-flash")
            .max_tokens(4000)
            .temperature(0.7)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build LLM: {}", e)))?;

        // Resolve prompt with execution context variables
        let resolved_prompt = self.resolve_template(&config.prompt_template, execution)?;

        let messages = vec![ChatMessage::user().content(&resolved_prompt).build()];

        let response_text = {
            let response = llm
                .chat(&messages)
                .await
                .map_err(|e| AppError::Internal(format!("LLM call failed: {}", e)))?;
            response.to_string()
        };

        Ok(NodeExecutionResult {
            status: ExecutionStatus::Completed,
            output_data: Some(json!({ "response": response_text })),
            error_message: None,
            next_nodes: self
                .get_next_node_ids(&node.id, &execution.workflow_id)
                .await?,
            execution_time_ms: 0,
        })
    }

    async fn execute_tool_call_node(
        &self,
        node: &WorkflowNode,
        config: &ToolCallNodeConfig,
        execution: &mut WorkflowExecution,
    ) -> Result<NodeExecutionResult, AppError> {
        // Find the tool by ID
        let tool = self
            .context
            .tools
            .iter()
            .find(|t| t.id == config.tool_id)
            .ok_or_else(|| {
                AppError::BadRequest(format!("Tool with ID '{}' not found", config.tool_id))
            })?;

        // Resolve parameters with execution context
        let resolved_parameters = self.resolve_parameters(
            &Value::Object(config.input_parameters.clone().into_iter().collect()),
            execution,
        )?;

        // Create tool call
        let tool_call = ToolCall {
            id: format!("workflow_tool_call_{}", node.id),
            name: format!("tool_{}", tool.name),
            arguments: resolved_parameters,
        };

        // Execute tool
        let tool_executor = ToolExecutor::new(
            self.context.clone(),
            self.app_state.clone(),
            self.conversation_history.clone(),
        );

        let tool_result = tool_executor.execute_tool_call(&tool_call).await?;

        // Store result in execution context
        execution
            .execution_context
            .tool_results
            .insert(node.id.clone(), tool_result.result.clone());

        Ok(NodeExecutionResult {
            status: ExecutionStatus::Completed,
            output_data: Some(tool_result.result),
            error_message: tool_result.error,
            next_nodes: self
                .get_next_node_ids(&node.id, &execution.workflow_id)
                .await?,
            execution_time_ms: 0,
        })
    }

    async fn execute_switch_node(
        &self,
        _: &WorkflowNode,
        config: &SwitchNodeConfig,
        execution: &WorkflowExecution,
    ) -> Result<NodeExecutionResult, AppError> {
        // Evaluate switch condition using LLM
        let switch_value = self
            .evaluate_switch_condition(&config.switch_condition, execution)
            .await?;

        // Find matching case
        let matching_case = config
            .cases
            .iter()
            .find(|case| self.matches_case_condition(&case.case_condition, &switch_value))
            .or_else(|| {
                config
                    .cases
                    .iter()
                    .find(|case| case.case_condition == "default")
            });

        let next_nodes = if let Some(_case) = matching_case {
            // For now, return empty next nodes since we don't have the exact field structure
            Vec::new()
        } else {
            Vec::new()
        };

        Ok(NodeExecutionResult {
            status: ExecutionStatus::Completed,
            output_data: Some(
                json!({ "switch_value": switch_value, "matched_case": matching_case.map(|c| &c.case_condition) }),
            ),
            error_message: None,
            next_nodes,
            execution_time_ms: 0,
        })
    }

    async fn execute_store_context_node(
        &self,
        node: &WorkflowNode,
        config: &StoreContextNodeConfig,
        execution: &mut WorkflowExecution,
    ) -> Result<NodeExecutionResult, AppError> {
        let resolved_data = if config.use_llm {
            self.generate_context_with_llm("context_key", execution)
                .await?
        } else {
            self.resolve_template(&config.context_data, execution)?
        };

        execution
            .execution_context
            .memory
            .insert("context_key".to_string(), json!(resolved_data));

        let _redis_key = format!(
            "workflow_context:{}:{}",
            execution.execution_id, "context_key"
        );
        // TODO: Implement Redis storage

        Ok(NodeExecutionResult {
            status: ExecutionStatus::Completed,
            output_data: Some(json!({ "stored_key": "context_key", "data": resolved_data })),
            error_message: None,
            next_nodes: self
                .get_next_node_ids(&node.id, &execution.workflow_id)
                .await?,
            execution_time_ms: 0,
        })
    }

    async fn execute_fetch_context_node(
        &self,
        node: &WorkflowNode,
        _config: &FetchContextNodeConfig,
        execution: &mut WorkflowExecution,
    ) -> Result<NodeExecutionResult, AppError> {
        // Fetch from execution memory first
        let context_data = if let Some(data) = execution.execution_context.memory.get("context_key")
        {
            data.clone()
        } else {
            // Try to fetch from Redis
            let _redis_key = format!(
                "workflow_context:{}:{}",
                execution.execution_id, "context_key"
            );
            // TODO: Implement Redis fetch
            json!(null)
        };

        // Store fetched data in variables for use by subsequent nodes
        execution
            .execution_context
            .variables
            .insert("fetched_context_key".to_string(), context_data.clone());

        Ok(NodeExecutionResult {
            status: ExecutionStatus::Completed,
            output_data: Some(json!({ "fetched_key": "context_key", "data": context_data })),
            error_message: None,
            next_nodes: self
                .get_next_node_ids(&node.id, &execution.workflow_id)
                .await?,
            execution_time_ms: 0,
        })
    }

    async fn execute_error_handler_node(
        &self,
        node: &WorkflowNode,
        _config: &ErrorHandlerNodeConfig,
        execution: &WorkflowExecution,
    ) -> Result<NodeExecutionResult, AppError> {
        // Error handlers are typically triggered by failures in other nodes
        // For now, just log the error and continue
        let error_info = json!({
            "handler_type": "generic",
            "error_message": "Error handled",
            "retry_count": 0,
            "fallback_action": "continue"
        });

        Ok(NodeExecutionResult {
            status: ExecutionStatus::Completed,
            output_data: Some(error_info),
            error_message: None,
            next_nodes: self
                .get_next_node_ids(&node.id, &execution.workflow_id)
                .await?,
            execution_time_ms: 0,
        })
    }

    // Helper methods

    async fn evaluate_condition(
        &self,
        condition: &str,
        execution: &WorkflowExecution,
    ) -> Result<bool, AppError> {
        // Use LLM to evaluate natural language conditions
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.0-flash")
            .max_tokens(100)
            .temperature(0.1)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build LLM: {}", e)))?;

        let context_info = serde_json::to_string_pretty(&execution.execution_context.variables)
            .unwrap_or_default();

        let prompt = format!(
            r#"Evaluate this condition based on the given context:

Condition: {condition}

Context:
{context_info}

Respond with only "true" or "false"."#
        );

        let messages = vec![ChatMessage::user().content(&prompt).build()];

        let response_text = {
            let response = llm
                .chat(&messages)
                .await
                .map_err(|e| AppError::Internal(format!("Condition evaluation failed: {}", e)))?;
            response.to_string()
        };

        Ok(response_text.trim().to_lowercase() == "true")
    }

    async fn evaluate_switch_condition(
        &self,
        condition: &str,
        execution: &WorkflowExecution,
    ) -> Result<String, AppError> {
        // Similar to condition evaluation but returns the actual value
        let resolved_condition = self.resolve_template(condition, execution)?;
        Ok(resolved_condition)
    }

    fn matches_case_condition(&self, case_condition: &str, switch_value: &str) -> bool {
        // Simple string matching for now
        case_condition == switch_value || case_condition == "default"
    }

    async fn generate_context_with_llm(
        &self,
        context_key: &str,
        execution: &WorkflowExecution,
    ) -> Result<String, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.0-flash")
            .max_tokens(1000)
            .temperature(0.7)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build LLM: {}", e)))?;

        let context_info =
            serde_json::to_string_pretty(&execution.execution_context).unwrap_or_default();

        let prompt = format!(
            r#"Generate context data for key '{context_key}' based on the current workflow execution state:

Execution Context:
{context_info}

Generate relevant context data that would be useful for this key."#
        );

        let messages = vec![ChatMessage::user().content(&prompt).build()];

        let response_text = {
            let response = llm
                .chat(&messages)
                .await
                .map_err(|e| AppError::Internal(format!("Context generation failed: {}", e)))?;
            response.to_string()
        };

        Ok(response_text)
    }

    fn resolve_template(
        &self,
        template: &str,
        execution: &WorkflowExecution,
    ) -> Result<String, AppError> {
        let mut resolved = template.to_string();

        // Replace variables in the format ${variable_name}
        for (key, value) in &execution.execution_context.variables {
            let placeholder = format!("${{{}}}", key);
            let value_str = match value {
                Value::String(s) => s.clone(),
                _ => serde_json::to_string(value).unwrap_or_default(),
            };
            resolved = resolved.replace(&placeholder, &value_str);
        }

        // Replace memory references
        for (key, value) in &execution.execution_context.memory {
            let placeholder = format!("${{memory.{}}}", key);
            let value_str = match value {
                Value::String(s) => s.clone(),
                _ => serde_json::to_string(value).unwrap_or_default(),
            };
            resolved = resolved.replace(&placeholder, &value_str);
        }

        Ok(resolved)
    }

    fn resolve_parameters(
        &self,
        parameters: &Value,
        execution: &WorkflowExecution,
    ) -> Result<Value, AppError> {
        match parameters {
            Value::String(s) => Ok(Value::String(self.resolve_template(s, execution)?)),
            Value::Object(obj) => {
                let mut resolved_obj = serde_json::Map::new();
                for (key, value) in obj {
                    resolved_obj.insert(key.clone(), self.resolve_parameters(value, execution)?);
                }
                Ok(Value::Object(resolved_obj))
            }
            Value::Array(arr) => {
                let mut resolved_arr = Vec::new();
                for value in arr {
                    resolved_arr.push(self.resolve_parameters(value, execution)?);
                }
                Ok(Value::Array(resolved_arr))
            }
            _ => Ok(parameters.clone()),
        }
    }

    async fn get_next_node_ids(
        &self,
        _current_node_id: &str,
        _workflow_id: &i64,
    ) -> Result<Vec<String>, AppError> {
        // This would typically query the workflow definition to find connected nodes
        // For now, return empty vector (end of workflow)
        Ok(Vec::new())
    }
}
