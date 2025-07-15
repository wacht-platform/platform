use chrono::Utc;
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;
use regex::Regex;
use serde_json::{Value, json};
use shared::dto::json::StreamEvent;
use shared::error::AppError;
use shared::models::{
    AiWorkflow, ConditionEvaluationType, ConditionType, ConversationContent, ExecutionContext,
    ExecutionStatus, MemoryEntry, NodeExecution, ResponseFormat, WorkflowEdge, WorkflowNode,
    WorkflowNodeType,
};
use shared::queries::Query;
use shared::state::AppState;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use tokio::sync::mpsc::Sender;

pub struct WorkflowExecutor {
    app_state: AppState,
}

impl WorkflowExecutor {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    pub async fn execute_workflow_task(
        &self,
        workflow_call: &shared::dto::json::WorkflowCall,
        workflows: &[AiWorkflow],
        _memories: &[MemoryEntry],
        channel: Sender<StreamEvent>,
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

        // Execute the full workflow
        let result = self
            .execute_workflow(workflow, workflow_call.inputs.clone(), channel)
            .await;

        match &result {
            Ok(_res) => {}
            Err(_e) => {}
        }

        result
    }

    pub async fn execute_workflow(
        &self,
        workflow: &AiWorkflow,
        input_data: Value,
        channel: Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        let mut execution_context = ExecutionContext::default();

        // Initialize variables with input data
        if let Some(input_obj) = input_data.as_object() {
            for (key, value) in input_obj {
                execution_context
                    .variables
                    .insert(key.clone(), value.clone());
            }
        }

        // Note: Workflow validation and context gathering is now handled in agent executor
        // This allows workflows to execute directly with provided context

        // Find the trigger node to start execution
        let trigger_node = workflow
            .workflow_definition
            .nodes
            .iter()
            .find(|node| matches!(node.node_type, WorkflowNodeType::Trigger(_)))
            .ok_or_else(|| AppError::BadRequest("No trigger node found in workflow".to_string()))?;

        // Execute workflow starting from trigger
        let output = self
            .execute_node_recursive(
                trigger_node,
                &workflow.workflow_definition.nodes,
                &workflow.workflow_definition.edges,
                &mut execution_context,
                channel.clone(),
                0,
            )
            .await?;

        Ok(json!({
            "workflow_id": workflow.id,
            "workflow_name": workflow.name,
            "execution_status": "completed",
            "output": output,
            "context": execution_context.variables,
            "node_executions": execution_context.node_executions,
        }))
    }

    fn execute_node_recursive<'a>(
        &'a self,
        node: &'a WorkflowNode,
        all_nodes: &'a [WorkflowNode],
        all_edges: &'a [WorkflowEdge],
        context: &'a mut ExecutionContext,
        channel: Sender<StreamEvent>,
        depth: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Value, AppError>> + Send + 'a>> {
        Box::pin(async move {
            // Prevent infinite recursion
            if depth > 100 {
                return Err(AppError::Internal(
                    "Workflow execution depth limit exceeded".to_string(),
                ));
            }

            // Record node execution start
            let mut node_execution = NodeExecution {
                node_id: node.id.clone(),
                status: ExecutionStatus::Running,
                started_at: Some(Utc::now()),
                completed_at: None,
                input_data: Some(json!(context.variables)),
                output_data: None,
                error_message: None,
                retry_count: 0,
            };

            context.current_node = Some(node.id.clone());

            // Execute the node based on its type
            let result = match &node.node_type {
                WorkflowNodeType::Trigger(config) => {
                    self.execute_trigger_node(config, context).await
                }
                WorkflowNodeType::Condition(config) => {
                    self.execute_condition_node(config, context).await
                }
                WorkflowNodeType::ErrorHandler(config) => {
                    self.execute_error_handler_node(
                        config,
                        all_nodes,
                        all_edges,
                        context,
                        channel.clone(),
                        depth,
                    )
                    .await
                }
                WorkflowNodeType::LLMCall(config) => {
                    self.execute_llm_call_node(config, context).await
                }
                WorkflowNodeType::Switch(config) => self.execute_switch_node(config, context).await,
                WorkflowNodeType::ToolCall(config) => {
                    self.execute_tool_call_node(config, context, channel.clone())
                        .await
                }
                WorkflowNodeType::StoreContext(config) => {
                    self.execute_store_context_node(config, context).await
                }
                WorkflowNodeType::FetchContext(config) => {
                    self.execute_fetch_context_node(config, context).await
                }
            };

            match result {
                Ok(output) => {
                    node_execution.status = ExecutionStatus::Completed;
                    node_execution.completed_at = Some(Utc::now());
                    node_execution.output_data = Some(output.clone());
                    context.node_executions.push(node_execution);

                    // Store output in context
                    context
                        .variables
                        .insert(format!("{}_output", node.id), output.clone());

                    // Find and execute next nodes
                    let next_edges: Vec<&WorkflowEdge> = all_edges
                        .iter()
                        .filter(|edge| edge.source == node.id)
                        .collect();

                    if next_edges.is_empty() {
                        // This is a terminal node
                        Ok(output)
                    } else {
                        // Execute next nodes based on conditions
                        let mut last_output = output;
                        for edge in next_edges {
                            if self.should_follow_edge(edge, &last_output, context)? {
                                if let Some(next_node) =
                                    all_nodes.iter().find(|n| n.id == edge.target)
                                {
                                    last_output = self
                                        .execute_node_recursive(
                                            next_node,
                                            all_nodes,
                                            all_edges,
                                            context,
                                            channel.clone(),
                                            depth + 1,
                                        )
                                        .await?;
                                }
                            }
                        }
                        Ok(last_output)
                    }
                }
                Err(e) => {
                    node_execution.status = ExecutionStatus::Failed;
                    node_execution.completed_at = Some(Utc::now());
                    node_execution.error_message = Some(e.to_string());
                    context.node_executions.push(node_execution);
                    Err(e)
                }
            }
        })
    }

    fn should_follow_edge(
        &self,
        edge: &WorkflowEdge,
        node_output: &Value,
        context: &ExecutionContext,
    ) -> Result<bool, AppError> {
        if let Some(condition) = &edge.condition {
            match condition.condition_type {
                ConditionType::Always => Ok(true),
                ConditionType::OnSuccess => {
                    // Check if the node output indicates success
                    // We could check for an "error" field or specific success indicators
                    Ok(!node_output.get("error").is_some())
                }
                ConditionType::OnError => {
                    // Check if the node output indicates an error
                    Ok(node_output.get("error").is_some())
                }
                ConditionType::OnCondition => {
                    // Create a temporary context with the node output available
                    let mut eval_context = context.clone();
                    eval_context
                        .variables
                        .insert("_last_output".to_string(), node_output.clone());

                    // Evaluate the expression with access to the last output
                    self.evaluate_expression(&condition.expression, &eval_context)
                }
            }
        } else {
            Ok(true) // No condition means always follow
        }
    }

    async fn execute_trigger_node(
        &self,
        config: &shared::models::TriggerNodeConfig,
        context: &ExecutionContext,
    ) -> Result<Value, AppError> {
        Ok(json!({
            "type": "trigger",
            "triggered": true,
            "description": config.description,
            "trigger_condition": config.condition,
            "context": context.variables,
        }))
    }

    async fn execute_condition_node(
        &self,
        config: &shared::models::ConditionNodeConfig,
        context: &ExecutionContext,
    ) -> Result<Value, AppError> {
        let result = match config.condition_type {
            ConditionEvaluationType::JavaScript => {
                // For security, we don't execute JavaScript directly
                // Instead, we evaluate simple expressions
                self.evaluate_expression(&config.expression, context)?
            }
            ConditionEvaluationType::JsonPath => {
                self.evaluate_jsonpath(&config.expression, context)?
            }
            ConditionEvaluationType::Simple => {
                self.evaluate_simple_condition(&config.expression, context)?
            }
        };

        Ok(json!({
            "type": "condition",
            "result": result,
            "expression": config.expression,
        }))
    }

    async fn execute_error_handler_node(
        &self,
        config: &shared::models::ErrorHandlerNodeConfig,
        all_nodes: &[WorkflowNode],
        all_edges: &[WorkflowEdge],
        context: &mut ExecutionContext,
        channel: Sender<StreamEvent>,
        depth: usize,
    ) -> Result<Value, AppError> {
        let mut retry_count = 0;
        let max_retries = if config.enable_retry {
            config.max_retries
        } else {
            0
        };

        loop {
            // Find and execute contained nodes
            let mut last_result = Ok(json!({}));
            for node_id in &config.contained_nodes {
                if let Some(node) = all_nodes.iter().find(|n| n.id == *node_id) {
                    match self
                        .execute_node_recursive(
                            node,
                            all_nodes,
                            all_edges,
                            context,
                            channel.clone(),
                            depth + 1,
                        )
                        .await
                    {
                        Ok(result) => last_result = Ok(result),
                        Err(e) => {
                            if config.log_errors {
                                // Send error information through channel for real-time feedback
                                let error_message = format!(
                                    "Workflow node '{}' failed on attempt {}: {}",
                                    &node.id,
                                    retry_count + 1,
                                    e
                                );
                            }

                            if retry_count < max_retries {
                                retry_count += 1;

                                tokio::time::sleep(tokio::time::Duration::from_secs(
                                    config.retry_delay_seconds as u64,
                                ))
                                .await;
                                break; // Break inner loop to retry
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
            }

            // If we completed all nodes successfully, return
            if last_result.is_ok() {
                return last_result;
            }
        }
    }

    async fn execute_llm_call_node(
        &self,
        config: &shared::models::LLMCallNodeConfig,
        context: &ExecutionContext,
    ) -> Result<Value, AppError> {
        // Render the prompt template with context variables
        let prompt = self.render_template_string(&config.prompt_template, context)?;

        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.5-pro")
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to create LLM: {}", e)))?;

        let messages = vec![ChatMessage::user().content(&prompt).build()];

        let response = llm
            .chat(&messages)
            .await
            .map_err(|e| AppError::Internal(format!("LLM call failed: {}", e)))?;

        let response_text = response.to_string();

        match config.response_format {
            ResponseFormat::Text => Ok(json!({
                "type": "llm_response",
                "format": "text",
                "content": response_text,
            })),
            ResponseFormat::Json => {
                // Try to parse as JSON
                serde_json::from_str(&response_text).or_else(|_| {
                    Ok(json!({
                        "type": "llm_response",
                        "format": "json",
                        "content": response_text,
                        "parse_error": "Failed to parse response as JSON"
                    }))
                })
            }
        }
    }

    async fn execute_switch_node(
        &self,
        config: &shared::models::SwitchNodeConfig,
        context: &ExecutionContext,
    ) -> Result<Value, AppError> {
        let switch_value = self
            .get_context_value(&config.switch_condition, context)
            .unwrap_or_else(|_| json!(config.switch_condition));

        for (index, case) in config.cases.iter().enumerate() {
            if self.matches_case(&switch_value, &case.case_condition)? {
                return Ok(json!({
                    "type": "switch",
                    "matched_case": index,
                    "case_label": case.case_label,
                    "switch_value": switch_value,
                }));
            }
        }

        if config.default_case {
            Ok(json!({
                "type": "switch",
                "matched_case": "default",
                "switch_value": switch_value,
            }))
        } else {
            Err(AppError::Internal(format!(
                "No matching case for switch value: {}",
                switch_value
            )))
        }
    }

    async fn execute_tool_call_node(
        &self,
        config: &shared::models::ToolCallNodeConfig,
        context: &ExecutionContext,
        channel: Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        // Get tool from database
        let tool = shared::queries::GetToolByIdQuery::new(config.tool_id)
            .execute(&self.app_state)
            .await?;

        let mut parameters = HashMap::new();
        for (key, value) in &config.input_parameters {
            let resolved_value = if let Some(str_val) = value.as_str() {
                if str_val.starts_with("{{") && str_val.ends_with("}}") {
                    // This is a template variable
                    let var_name = str_val
                        .trim_start_matches("{{")
                        .trim_end_matches("}}")
                        .trim();
                    context
                        .variables
                        .get(var_name)
                        .cloned()
                        .unwrap_or(value.clone())
                } else {
                    value.clone()
                }
            } else {
                value.clone()
            };
            parameters.insert(key.clone(), resolved_value);
        }

        // Create tool executor and execute
        let tool_executor = super::tool_executor::ToolExecutor::new(self.app_state.clone());

        // Execute the tool immediately and return the result
        let result = tool_executor
            .execute_tool_immediately(&tool, json!(parameters))
            .await?;

        Ok(result)
    }

    async fn execute_store_context_node(
        &self,
        config: &shared::models::StoreContextNodeConfig,
        context: &mut ExecutionContext,
    ) -> Result<Value, AppError> {
        let data = if config.use_llm {
            // Use LLM to process the data first
            let prompt = self.render_template_string(&config.context_data, context)?;

            let api_key = std::env::var("GEMINI_API_KEY")
                .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

            let llm = LLMBuilder::new()
                .backend(LLMBackend::Google)
                .api_key(&api_key)
                .model("gemini-2.5-pro")
                .build()
                .map_err(|e| AppError::Internal(format!("Failed to create LLM: {}", e)))?;

            let messages = vec![ChatMessage::user().content(&prompt).build()];

            let response = llm
                .chat(&messages)
                .await
                .map_err(|e| AppError::Internal(format!("LLM call failed: {}", e)))?;

            json!(response.to_string())
        } else {
            // Store raw data
            json!(config.context_data)
        };

        // Generate a unique key for this data
        let key = format!("stored_context_{}", Utc::now().timestamp_millis());
        context.variables.insert(key.clone(), data.clone());

        Ok(json!({
            "type": "store_context",
            "key": key,
            "data": data,
        }))
    }

    async fn execute_fetch_context_node(
        &self,
        config: &shared::models::FetchContextNodeConfig,
        context: &ExecutionContext,
    ) -> Result<Value, AppError> {
        let key = config.context_data.trim();
        let data = context
            .variables
            .get(key)
            .cloned()
            .unwrap_or_else(|| json!(null));

        if config.use_llm && !data.is_null() {
            // Use LLM to process the fetched data
            let prompt = format!("Process this data: {}", data);

            let api_key = std::env::var("GEMINI_API_KEY")
                .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

            let llm = LLMBuilder::new()
                .backend(LLMBackend::Google)
                .api_key(&api_key)
                .model("gemini-2.5-pro")
                .build()
                .map_err(|e| AppError::Internal(format!("Failed to create LLM: {}", e)))?;

            let messages = vec![ChatMessage::user().content(&prompt).build()];

            let response = llm
                .chat(&messages)
                .await
                .map_err(|e| AppError::Internal(format!("LLM call failed: {}", e)))?;

            Ok(json!({
                "type": "fetch_context",
                "key": key,
                "data": data,
                "processed": response.to_string(),
            }))
        } else {
            Ok(json!({
                "type": "fetch_context",
                "key": key,
                "data": data,
            }))
        }
    }

    // Helper methods
    fn evaluate_expression(
        &self,
        expression: &str,
        context: &ExecutionContext,
    ) -> Result<bool, AppError> {
        // Simple expression evaluation (safe subset)
        // Supports: ==, !=, >, <, >=, <=, &&, ||
        let expr = expression.trim();

        // Handle boolean literals
        if expr == "true" {
            return Ok(true);
        }
        if expr == "false" {
            return Ok(false);
        }

        // Handle simple comparisons
        if let Some((left, right)) = expr.split_once("==") {
            let left_val = self.get_context_value(left.trim(), context)?;
            let right_val = self.get_context_value(right.trim(), context)?;
            return Ok(left_val == right_val);
        }

        if let Some((left, right)) = expr.split_once("!=") {
            let left_val = self.get_context_value(left.trim(), context)?;
            let right_val = self.get_context_value(right.trim(), context)?;
            return Ok(left_val != right_val);
        }

        // Handle numeric comparisons
        if let Some((left, right)) = expr.split_once(">=") {
            let left_val = self.get_numeric_value(left.trim(), context)?;
            let right_val = self.get_numeric_value(right.trim(), context)?;
            return Ok(left_val >= right_val);
        }

        if let Some((left, right)) = expr.split_once("<=") {
            let left_val = self.get_numeric_value(left.trim(), context)?;
            let right_val = self.get_numeric_value(right.trim(), context)?;
            return Ok(left_val <= right_val);
        }

        if let Some((left, right)) = expr.split_once(">") {
            let left_val = self.get_numeric_value(left.trim(), context)?;
            let right_val = self.get_numeric_value(right.trim(), context)?;
            return Ok(left_val > right_val);
        }

        if let Some((left, right)) = expr.split_once("<") {
            let left_val = self.get_numeric_value(left.trim(), context)?;
            let right_val = self.get_numeric_value(right.trim(), context)?;
            return Ok(left_val < right_val);
        }

        // Handle logical operators
        if let Some((left, right)) = expr.split_once("&&") {
            let left_result = self.evaluate_expression(left.trim(), context)?;
            let right_result = self.evaluate_expression(right.trim(), context)?;
            return Ok(left_result && right_result);
        }

        if let Some((left, right)) = expr.split_once("||") {
            let left_result = self.evaluate_expression(left.trim(), context)?;
            let right_result = self.evaluate_expression(right.trim(), context)?;
            return Ok(left_result || right_result);
        }

        // Default to false for unsupported expressions
        Ok(false)
    }

    fn evaluate_jsonpath(&self, path: &str, context: &ExecutionContext) -> Result<bool, AppError> {
        // Simple JSONPath evaluation
        let value = self.get_context_value(path, context)?;
        Ok(!value.is_null() && value != json!(false))
    }

    fn evaluate_simple_condition(
        &self,
        condition: &str,
        context: &ExecutionContext,
    ) -> Result<bool, AppError> {
        // Evaluate simple conditions like "variable_name" or "!variable_name"
        let condition = condition.trim();
        if condition.starts_with('!') {
            let var_name = condition.trim_start_matches('!').trim();
            let value = self.get_context_value(var_name, context)?;
            Ok(value.is_null() || value == json!(false))
        } else {
            let value = self.get_context_value(condition, context)?;
            Ok(!value.is_null() && value != json!(false))
        }
    }

    fn get_context_value(&self, path: &str, context: &ExecutionContext) -> Result<Value, AppError> {
        let path = path.trim();

        // Handle literal values
        if path.starts_with('"') && path.ends_with('"') {
            return Ok(json!(path.trim_matches('"')));
        }

        if let Ok(num) = path.parse::<f64>() {
            return Ok(json!(num));
        }

        if path == "true" || path == "false" {
            return Ok(json!(path == "true"));
        }

        // Handle context variable access
        if let Some(value) = context.variables.get(path) {
            return Ok(value.clone());
        }

        // Handle nested path access (e.g., "user.name")
        let parts: Vec<&str> = path.split('.').collect();
        if parts.len() > 1 {
            if let Some(base_value) = context.variables.get(parts[0]) {
                let mut current = base_value;
                for part in &parts[1..] {
                    if let Some(next) = current.get(part) {
                        current = next;
                    } else {
                        return Ok(json!(null));
                    }
                }
                return Ok(current.clone());
            }
        }

        Ok(json!(null))
    }

    fn get_numeric_value(&self, path: &str, context: &ExecutionContext) -> Result<f64, AppError> {
        let value = self.get_context_value(path, context)?;

        match &value {
            Value::Number(n) => n
                .as_f64()
                .ok_or_else(|| AppError::Internal(format!("Could not convert {} to f64", value))),
            Value::String(s) => s
                .parse::<f64>()
                .map_err(|_| AppError::Internal(format!("Could not parse '{}' as number", s))),
            _ => Err(AppError::Internal(format!(
                "Value '{}' is not a number",
                value
            ))),
        }
    }

    fn render_template_string(
        &self,
        template: &str,
        context: &ExecutionContext,
    ) -> Result<String, AppError> {
        let mut result = template.to_string();

        // Simple template rendering - replace {{variable}} with values
        let re = Regex::new(r"\{\{(\s*[a-zA-Z_][a-zA-Z0-9_.]*\s*)\}\}")
            .map_err(|e| AppError::Internal(format!("Invalid regex: {}", e)))?;

        for cap in re.captures_iter(template) {
            if let Some(var_name) = cap.get(1) {
                let var_name = var_name.as_str().trim();
                let value = self.get_context_value(var_name, context)?;
                let value_str = match value {
                    Value::String(s) => s,
                    _ => value.to_string(),
                };
                result = result.replace(&cap[0], &value_str);
            }
        }

        Ok(result)
    }

    fn matches_case(&self, value: &Value, case_condition: &str) -> Result<bool, AppError> {
        let case_val = json!(case_condition);

        // Direct equality check
        if value == &case_val {
            return Ok(true);
        }

        // String comparison with patterns
        if let (Some(val_str), Some(case_str)) = (value.as_str(), case_val.as_str()) {
            // Check for wildcards
            if case_str.contains('*') {
                let pattern = case_str.replace("*", ".*");
                let re = Regex::new(&format!("^{}$", pattern))
                    .map_err(|e| AppError::Internal(format!("Invalid regex pattern: {}", e)))?;
                return Ok(re.is_match(val_str));
            }
        }

        Ok(false)
    }
}
