use super::{AgentContext, ToolCall, ToolExecutor, WorkflowEngine};
use chrono::{DateTime, Utc};
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;
use serde_json::{Value, json};
use shared::error::AppError;
use shared::state::AppState;
use shared::commands::{Command, GenerateEmbeddingCommand, StoreConversationEmbeddingCommand};
use std::collections::HashMap;

#[derive(Clone)]
pub struct TaskManager {
    pub execution_id: String,
    pub tasks: Vec<AgentTask>,
    pub current_task_id: Option<String>,
    pub completed_tasks: Vec<String>,
    pub failed_tasks: Vec<String>,
    pub execution_context: TaskExecutionContext,
    pub app_state: AppState,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentTask {
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub dependencies: Vec<String>,
    pub reasoning: Option<TaskReasoning>,
    pub actions: Vec<TaskAction>,
    pub estimated_duration_minutes: Option<u32>,
    pub actual_duration_minutes: Option<u32>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub progress_percentage: u8,
    pub subtasks: Vec<AgentTask>,
    pub context_requirements: Vec<String>,
    pub output_artifacts: Vec<TaskArtifact>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TaskStatus {
    NotStarted,
    InProgress,
    Blocked,
    WaitingForInput,
    Completed,
    Failed,
    Cancelled,
    Paused,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TaskPriority {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskReasoning {
    pub analysis: String,
    pub approach: String,
    pub expected_outcome: String,
    pub potential_challenges: Vec<String>,
    pub success_criteria: Vec<String>,
    pub reasoning_steps: Vec<ReasoningStep>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReasoningStep {
    pub step_type: ReasoningStepType,
    pub description: String,
    pub evidence: Vec<String>,
    pub conclusion: String,
    pub confidence_score: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ReasoningStepType {
    ProblemAnalysis,
    ContextGathering,
    OptionEvaluation,
    DecisionMaking,
    PlanFormulation,
    RiskAssessment,
    Validation,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskAction {
    pub id: String,
    pub action_type: TaskActionType,
    pub description: String,
    pub parameters: Value,
    pub status: TaskActionStatus,
    pub result: Option<Value>,
    pub error_message: Option<String>,
    pub execution_order: u32,
    pub retry_count: u32,
    pub max_retries: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TaskActionType {
    ToolExecution,
    WorkflowExecution,
    LLMCall,
    ContextSearch,
    ContextStore,
    UserInteraction,
    DataValidation,
    FileOperation,
    APICall,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TaskActionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskArtifact {
    pub id: String,
    pub artifact_type: ArtifactType,
    pub name: String,
    pub description: String,
    pub content: Value,
    pub metadata: HashMap<String, Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ArtifactType {
    Code,
    Documentation,
    Configuration,
    Data,
    Analysis,
    Report,
    Plan,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskExecutionContext {
    pub variables: HashMap<String, Value>,
    pub shared_memory: HashMap<String, Value>,
    pub tool_results: HashMap<String, Value>,
    pub workflow_outputs: HashMap<String, Value>,
    pub user_inputs: HashMap<String, Value>,
    pub execution_metadata: HashMap<String, Value>,
}

impl TaskManager {
    pub fn new(execution_id: String, app_state: AppState) -> Self {
        Self {
            execution_id,
            tasks: Vec::new(),
            current_task_id: None,
            completed_tasks: Vec::new(),
            failed_tasks: Vec::new(),
            execution_context: TaskExecutionContext {
                variables: HashMap::new(),
                shared_memory: HashMap::new(),
                tool_results: HashMap::new(),
                workflow_outputs: HashMap::new(),
                user_inputs: HashMap::new(),
                execution_metadata: HashMap::new(),
            },
            app_state,
        }
    }

    pub async fn analyze_and_create_task_plan(
        &mut self,
        user_request: &str,
        agent_context: &AgentContext,
        _app_state: &AppState,
    ) -> Result<(), AppError> {
        // Use LLM to analyze the request and create a comprehensive task plan
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.0-flash")
            .max_tokens(8000)
            .temperature(0.3)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build LLM: {}", e)))?;

        let available_capabilities = self.get_available_capabilities(agent_context);

        let prompt = format!(
            r#"You are an expert AI task planner. Analyze this user request and create a comprehensive task breakdown that follows the same pattern as Claude's task management.

User Request: {user_request}

Available Capabilities:
{available_capabilities}

Create a detailed task plan with the following structure:

1. **MAIN GOAL ANALYSIS**: Break down the core objective
2. **TASK DECOMPOSITION**: Create meaningful tasks (15-20 minutes each)
3. **DEPENDENCY MAPPING**: Identify task dependencies and execution order
4. **REASONING FOR EACH TASK**: Provide analysis, approach, and expected outcomes
5. **ACTION PLANNING**: Define specific actions for each task
6. **SUCCESS CRITERIA**: Define how to measure task completion

Return a JSON structure with this format:
{{
  "main_goal": "Clear description of the main objective",
  "analysis": "Detailed analysis of the request",
  "tasks": [
    {{
      "name": "Task name",
      "description": "Detailed description",
      "priority": "High|Medium|Low|Critical",
      "estimated_duration_minutes": 20,
      "dependencies": ["task_id_1", "task_id_2"],
      "reasoning": {{
        "analysis": "Why this task is needed",
        "approach": "How to accomplish it",
        "expected_outcome": "What success looks like",
        "potential_challenges": ["challenge1", "challenge2"],
        "success_criteria": ["criteria1", "criteria2"]
      }},
      "actions": [
        {{
          "type": "ToolExecution|WorkflowExecution|LLMCall|ContextSearch|UserInteraction",
          "description": "What this action does",
          "parameters": {{}},
          "execution_order": 1
        }}
      ],
      "context_requirements": ["context1", "context2"],
      "subtasks": []
    }}
  ],
  "execution_strategy": "Overall approach to executing the plan",
  "risk_assessment": "Potential risks and mitigation strategies"
}}

Focus on creating tasks that represent meaningful units of work, similar to how Claude breaks down complex requests."#,
            user_request = user_request,
            available_capabilities = available_capabilities
        );

        let messages = vec![ChatMessage::user().content(&prompt).build()];

        // Get response and convert to string in a separate scope
        let response_text = {
            let response = llm
                .chat(&messages)
                .await
                .map_err(|e| AppError::Internal(format!("Failed to analyze task plan: {}", e)))?;
            response.to_string()
        };

        // Parse the response and create tasks
        self.parse_and_create_tasks(&response_text).await?;

        Ok(())
    }

    fn get_available_capabilities(&self, agent_context: &AgentContext) -> String {
        let tools = agent_context
            .tools
            .iter()
            .map(|t| {
                format!(
                    "- {}: {}",
                    t.name,
                    t.description.as_deref().unwrap_or("No description")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let workflows = agent_context
            .workflows
            .iter()
            .map(|w| {
                format!(
                    "- {}: {}",
                    w.name,
                    w.description.as_deref().unwrap_or("No description")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let knowledge_bases = agent_context
            .knowledge_bases
            .iter()
            .map(|kb| {
                format!(
                    "- {}: {}",
                    kb.name,
                    kb.description.as_deref().unwrap_or("No description")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"Tools Available:
{tools}

Workflows Available:
{workflows}

Knowledge Bases Available:
{knowledge_bases}

Core Capabilities:
- Advanced reasoning and analysis
- Multi-step task execution
- Context management and memory
- Real-time communication
- Error handling and recovery
- Parallel task execution
- Dynamic parameter resolution"#,
            tools = tools,
            workflows = workflows,
            knowledge_bases = knowledge_bases
        )
    }

    async fn parse_and_create_tasks(&mut self, response: &str) -> Result<(), AppError> {
        // Parse JSON response and create AgentTask objects
        if let Ok(parsed) = serde_json::from_str::<Value>(response) {
            if let Some(tasks_array) = parsed.get("tasks").and_then(|t| t.as_array()) {
                for (index, task_data) in tasks_array.iter().enumerate() {
                    let task = self.create_task_from_json(task_data, index)?;
                    self.tasks.push(task);
                }
            }

            // Store execution metadata
            if let Some(main_goal) = parsed.get("main_goal").and_then(|g| g.as_str()) {
                self.execution_context
                    .execution_metadata
                    .insert("main_goal".to_string(), json!(main_goal));
            }

            if let Some(analysis) = parsed.get("analysis").and_then(|a| a.as_str()) {
                self.execution_context
                    .execution_metadata
                    .insert("analysis".to_string(), json!(analysis));
            }
        }

        Ok(())
    }

    fn create_task_from_json(
        &self,
        task_data: &Value,
        index: usize,
    ) -> Result<AgentTask, AppError> {
        let task_id = format!("task_{}", index);
        let name = task_data
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("Unnamed Task")
            .to_string();
        let description = task_data
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();

        let priority = match task_data
            .get("priority")
            .and_then(|p| p.as_str())
            .unwrap_or("Medium")
        {
            "Critical" => TaskPriority::Critical,
            "High" => TaskPriority::High,
            "Low" => TaskPriority::Low,
            _ => TaskPriority::Medium,
        };

        let estimated_duration = task_data
            .get("estimated_duration_minutes")
            .and_then(|d| d.as_u64())
            .map(|d| d as u32);

        let dependencies = task_data
            .get("dependencies")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let reasoning = if let Some(reasoning_data) = task_data.get("reasoning") {
            Some(TaskReasoning {
                analysis: reasoning_data
                    .get("analysis")
                    .and_then(|a| a.as_str())
                    .unwrap_or("")
                    .to_string(),
                approach: reasoning_data
                    .get("approach")
                    .and_then(|a| a.as_str())
                    .unwrap_or("")
                    .to_string(),
                expected_outcome: reasoning_data
                    .get("expected_outcome")
                    .and_then(|e| e.as_str())
                    .unwrap_or("")
                    .to_string(),
                potential_challenges: reasoning_data
                    .get("potential_challenges")
                    .and_then(|c| c.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                success_criteria: reasoning_data
                    .get("success_criteria")
                    .and_then(|c| c.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                reasoning_steps: Vec::new(), // Will be populated during execution
            })
        } else {
            None
        };

        let actions =
            if let Some(actions_array) = task_data.get("actions").and_then(|a| a.as_array()) {
                actions_array
                    .iter()
                    .enumerate()
                    .map(|(action_index, action_data)| TaskAction {
                        id: format!("action_{}_{}", index, action_index),
                        action_type: self.parse_action_type(
                            action_data
                                .get("type")
                                .and_then(|t| t.as_str())
                                .unwrap_or("LLMCall"),
                        ),
                        description: action_data
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .to_string(),
                        parameters: action_data.get("parameters").cloned().unwrap_or(json!({})),
                        status: TaskActionStatus::Pending,
                        result: None,
                        error_message: None,
                        execution_order: action_data
                            .get("execution_order")
                            .and_then(|o| o.as_u64())
                            .unwrap_or(1) as u32,
                        retry_count: 0,
                        max_retries: 3,
                    })
                    .collect()
            } else {
                Vec::new()
            };

        let context_requirements = task_data
            .get("context_requirements")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        Ok(AgentTask {
            id: task_id,
            name,
            description,
            status: TaskStatus::NotStarted,
            priority,
            dependencies,
            reasoning,
            actions,
            estimated_duration_minutes: estimated_duration,
            actual_duration_minutes: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            progress_percentage: 0,
            subtasks: Vec::new(),
            context_requirements,
            output_artifacts: Vec::new(),
        })
    }

    fn parse_action_type(&self, type_str: &str) -> TaskActionType {
        match type_str {
            "ToolExecution" => TaskActionType::ToolExecution,
            "WorkflowExecution" => TaskActionType::WorkflowExecution,
            "ContextSearch" => TaskActionType::ContextSearch,
            "ContextStore" => TaskActionType::ContextStore,
            "UserInteraction" => TaskActionType::UserInteraction,
            "DataValidation" => TaskActionType::DataValidation,
            "FileOperation" => TaskActionType::FileOperation,
            "APICall" => TaskActionType::APICall,
            _ => TaskActionType::LLMCall,
        }
    }

    pub async fn execute_task_plan<F>(
        &mut self,
        agent_context: &AgentContext,
        app_state: &AppState,
        mut progress_callback: F,
    ) -> Result<(), AppError>
    where
        F: FnMut(&str, u8) + Send,
    {
        // Execute tasks in dependency order
        let execution_order = self.calculate_execution_order()?;

        for task_id in execution_order {
            if let Some(task_index) = self.tasks.iter().position(|t| t.id == task_id) {
                self.current_task_id = Some(task_id.clone());

                // Update task status
                self.tasks[task_index].status = TaskStatus::InProgress;
                self.tasks[task_index].started_at = Some(Utc::now());

                // Execute the task
                match self
                    .execute_single_task(
                        task_index,
                        agent_context,
                        app_state,
                        &mut progress_callback,
                    )
                    .await
                {
                    Ok(_) => {
                        self.tasks[task_index].status = TaskStatus::Completed;
                        self.tasks[task_index].completed_at = Some(Utc::now());
                        self.tasks[task_index].progress_percentage = 100;
                        self.completed_tasks.push(task_id.clone());

                        progress_callback(
                            &format!("✅ Completed task: {}", self.tasks[task_index].name),
                            self.calculate_overall_progress(),
                        );
                    }
                    Err(e) => {
                        self.tasks[task_index].status = TaskStatus::Failed;
                        self.tasks[task_index].completed_at = Some(Utc::now());
                        self.failed_tasks.push(task_id.clone());

                        progress_callback(
                            &format!(
                                "❌ Failed task: {} - Error: {}",
                                self.tasks[task_index].name, e
                            ),
                            self.calculate_overall_progress(),
                        );

                        // Decide whether to continue or stop based on task criticality
                        if self.tasks[task_index].priority == TaskPriority::Critical {
                            return Err(e);
                        }
                    }
                }
            }
        }

        self.current_task_id = None;
        Ok(())
    }

    async fn execute_single_task<F>(
        &mut self,
        task_index: usize,
        agent_context: &AgentContext,
        app_state: &AppState,
        progress_callback: &mut F,
    ) -> Result<(), AppError>
    where
        F: FnMut(&str, u8) + Send,
    {
        let task = &self.tasks[task_index].clone();

        progress_callback(
            &format!("🔄 Starting task: {}", task.name),
            self.calculate_overall_progress(),
        );

        // Execute reasoning phase
        if let Some(reasoning) = &task.reasoning {
            progress_callback(
                &format!("🧠 Reasoning: {}", reasoning.analysis),
                self.calculate_overall_progress(),
            );
        }

        // Execute actions in order
        let mut action_results = Vec::new();
        for (action_index, action) in task.actions.iter().enumerate() {
            progress_callback(
                &format!("⚡ Executing action: {}", action.description),
                self.calculate_overall_progress(),
            );

            match self
                .execute_task_action(action, agent_context, app_state)
                .await
            {
                Ok(result) => {
                    action_results.push(result.clone());
                    self.tasks[task_index].actions[action_index].status =
                        TaskActionStatus::Completed;
                    self.tasks[task_index].actions[action_index].result = Some(result);
                }
                Err(e) => {
                    self.tasks[task_index].actions[action_index].status = TaskActionStatus::Failed;
                    self.tasks[task_index].actions[action_index].error_message =
                        Some(e.to_string());

                    // Retry logic
                    if self.tasks[task_index].actions[action_index].retry_count
                        < self.tasks[task_index].actions[action_index].max_retries
                    {
                        self.tasks[task_index].actions[action_index].retry_count += 1;
                        progress_callback(
                            &format!(
                                "🔄 Retrying action: {} (attempt {})",
                                action.description,
                                self.tasks[task_index].actions[action_index].retry_count + 1
                            ),
                            self.calculate_overall_progress(),
                        );
                        // Implement retry with exponential backoff
                        let delay_ms = 1000
                            * (2_u64.pow(
                                self.tasks[task_index].actions[action_index].retry_count as u32,
                            ));
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;

                        // Re-execute the action
                        match self
                            .execute_task_action(action, agent_context, app_state)
                            .await
                        {
                            Ok(result) => {
                                self.tasks[task_index].actions[action_index].status =
                                    TaskActionStatus::Completed;
                                self.tasks[task_index].actions[action_index].result =
                                    Some(result.clone());
                                action_results.push(result);
                                break; // Exit retry loop on success
                            }
                            Err(_) => {
                                // Continue to next retry or fail
                                continue;
                            }
                        }
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        // Store task results in execution context
        self.execution_context
            .tool_results
            .insert(task.id.clone(), json!(action_results));

        // Update progress
        self.tasks[task_index].progress_percentage = 100;

        Ok(())
    }

    async fn execute_task_action(
        &self,
        action: &TaskAction,
        agent_context: &AgentContext,
        app_state: &AppState,
    ) -> Result<Value, AppError> {
        match action.action_type {
            TaskActionType::ToolExecution => {
                self.execute_tool_action(action, agent_context, app_state)
                    .await
            }
            TaskActionType::WorkflowExecution => {
                self.execute_workflow_action(action, agent_context, app_state)
                    .await
            }
            TaskActionType::LLMCall => self.execute_llm_action(action).await,
            TaskActionType::ContextSearch => {
                self.execute_context_search_action(action, agent_context, app_state)
                    .await
            }
            TaskActionType::ContextStore => self.execute_context_store_action(action).await,
            TaskActionType::UserInteraction => self.execute_user_interaction_action(action).await,
            TaskActionType::DataValidation => self.execute_data_validation_action(action).await,
            TaskActionType::FileOperation => self.execute_file_operation_action(action).await,
            TaskActionType::APICall => self.execute_api_call_action(action).await,
        }
    }

    async fn execute_tool_action(
        &self,
        action: &TaskAction,
        agent_context: &AgentContext,
        app_state: &AppState,
    ) -> Result<Value, AppError> {
        // Extract tool name and parameters from action
        let tool_name = action
            .parameters
            .get("tool_name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| AppError::BadRequest("Tool name not specified".to_string()))?;

        let tool_params = action
            .parameters
            .get("parameters")
            .cloned()
            .unwrap_or(json!({}));

        // Create tool call
        let tool_call = ToolCall {
            id: action.id.clone(),
            name: tool_name.to_string(),
            arguments: tool_params,
        };

        // Execute tool using tool executor
        let tool_executor = ToolExecutor::new(
            agent_context.clone(),
            app_state.clone(),
            Vec::new(), // Empty conversation history for now
        );

        let result = tool_executor.execute_tool_call(&tool_call).await?;
        Ok(result.result)
    }

    async fn execute_workflow_action(
        &self,
        action: &TaskAction,
        agent_context: &AgentContext,
        app_state: &AppState,
    ) -> Result<Value, AppError> {
        // Extract workflow ID and input data
        let workflow_id = action
            .parameters
            .get("workflow_id")
            .and_then(|id| id.as_i64())
            .ok_or_else(|| AppError::BadRequest("Workflow ID not specified".to_string()))?;

        let input_data = action
            .parameters
            .get("input_data")
            .cloned()
            .unwrap_or(json!({}));

        // Find workflow
        let workflow = agent_context
            .workflows
            .iter()
            .find(|w| w.id == workflow_id)
            .ok_or_else(|| {
                AppError::BadRequest(format!("Workflow with ID {} not found", workflow_id))
            })?;

        // Execute workflow using workflow engine
        let workflow_engine =
            WorkflowEngine::new(agent_context.clone(), app_state.clone(), Vec::new());

        let execution = workflow_engine
            .execute_workflow(workflow, input_data, None)
            .await?;
        Ok(json!(execution.execution_context.output_data))
    }

    async fn execute_llm_action(&self, action: &TaskAction) -> Result<Value, AppError> {
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

        let prompt = action
            .parameters
            .get("prompt")
            .and_then(|p| p.as_str())
            .ok_or_else(|| {
                AppError::BadRequest("Prompt not specified for LLM action".to_string())
            })?;

        // Resolve prompt with execution context
        let resolved_prompt = self.resolve_template(prompt);

        let messages = vec![ChatMessage::user().content(&resolved_prompt).build()];
        let response_text = {
            let response = llm
                .chat(&messages)
                .await
                .map_err(|e| AppError::Internal(format!("LLM call failed: {}", e)))?;
            response.to_string()
        };

        Ok(json!({ "response": response_text }))
    }

    async fn execute_context_search_action(
        &self,
        action: &TaskAction,
        agent_context: &AgentContext,
        app_state: &AppState,
    ) -> Result<Value, AppError> {
        let query = action
            .parameters
            .get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| {
                AppError::BadRequest("Query not specified for context search".to_string())
            })?;

        // Use the context engine for actual search
        let context_engine =
            super::context_engine::ContextEngine::new(agent_context.clone(), app_state.clone())?;

        let results = context_engine.search(query).await?;
        Ok(results)
    }

    async fn execute_context_store_action(&self, action: &TaskAction) -> Result<Value, AppError> {
        let key = action
            .parameters
            .get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| {
                AppError::BadRequest("Key not specified for context store".to_string())
            })?;

        let data = action.parameters.get("data").ok_or_else(|| {
            AppError::BadRequest("Data not specified for context store".to_string())
        })?;

        let content = serde_json::to_string(data)
            .map_err(|e| AppError::Internal(format!("Failed to serialize data: {}", e)))?;

        let embedding = GenerateEmbeddingCommand::new(content.clone()).execute(&self.app_state).await?;

        let execution_context_id = self.get_execution_context_id().await?;

        let context_id = shared::utils::snowflake::generate_id();
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("action_id".to_string(), json!(action.id));
        metadata.insert("task_type".to_string(), json!("context_store"));
        metadata.insert("key".to_string(), json!(key));
        metadata.insert("context_type".to_string(), json!("task_context"));

        StoreConversationEmbeddingCommand::new(
            context_id,
            self.get_deployment_id().await?,
            execution_context_id,
            self.get_agent_id().await?,
            "context".to_string(),
            content,
            embedding,
        ).execute(&self.app_state).await?;

        Ok(json!({
            "key": key,
            "data": data,
            "context_id": context_id,
            "message": "Context stored successfully in Qdrant"
        }))
    }

    async fn execute_user_interaction_action(
        &self,
        action: &TaskAction,
    ) -> Result<Value, AppError> {
        let message = action
            .parameters
            .get("message")
            .and_then(|m| m.as_str())
            .ok_or_else(|| {
                AppError::BadRequest("Message not specified for user interaction".to_string())
            })?;

        // TODO: Implement actual user interaction via WebSocket
        // For now, return the message that would be sent to user
        Ok(json!({
            "type": "user_interaction",
            "message": message,
            "status": "pending_user_response"
        }))
    }

    async fn execute_data_validation_action(&self, action: &TaskAction) -> Result<Value, AppError> {
        let data = action
            .parameters
            .get("data")
            .ok_or_else(|| AppError::BadRequest("Data not specified for validation".to_string()))?;

        let validation_rules = action
            .parameters
            .get("rules")
            .and_then(|r| r.as_array())
            .ok_or_else(|| AppError::BadRequest("Validation rules not specified".to_string()))?;

        // TODO: Implement actual data validation logic
        // For now, return mock validation result
        Ok(json!({
            "data": data,
            "rules": validation_rules,
            "valid": true,
            "errors": []
        }))
    }

    async fn execute_file_operation_action(&self, action: &TaskAction) -> Result<Value, AppError> {
        let operation = action
            .parameters
            .get("operation")
            .and_then(|o| o.as_str())
            .ok_or_else(|| AppError::BadRequest("File operation not specified".to_string()))?;

        // TODO: Implement actual file operations
        // For now, return mock result
        Ok(json!({
            "operation": operation,
            "status": "completed",
            "message": "File operation completed successfully"
        }))
    }

    async fn execute_api_call_action(&self, action: &TaskAction) -> Result<Value, AppError> {
        let url = action
            .parameters
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| AppError::BadRequest("URL not specified for API call".to_string()))?;

        let method = action
            .parameters
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("GET");

        // TODO: Implement actual API calls
        // For now, return mock result
        Ok(json!({
            "url": url,
            "method": method,
            "status": "completed",
            "response": "Mock API response"
        }))
    }

    fn calculate_execution_order(&self) -> Result<Vec<String>, AppError> {
        // Simple topological sort for task dependencies
        let mut order = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut temp_visited = std::collections::HashSet::new();

        for task in &self.tasks {
            if !visited.contains(&task.id) {
                self.visit_task(&task.id, &mut visited, &mut temp_visited, &mut order)?;
            }
        }

        order.reverse();
        Ok(order)
    }

    fn visit_task(
        &self,
        task_id: &str,
        visited: &mut std::collections::HashSet<String>,
        temp_visited: &mut std::collections::HashSet<String>,
        order: &mut Vec<String>,
    ) -> Result<(), AppError> {
        if temp_visited.contains(task_id) {
            return Err(AppError::BadRequest(
                "Circular dependency detected in tasks".to_string(),
            ));
        }

        if visited.contains(task_id) {
            return Ok(());
        }

        temp_visited.insert(task_id.to_string());

        if let Some(task) = self.tasks.iter().find(|t| t.id == task_id) {
            for dep_id in &task.dependencies {
                self.visit_task(dep_id, visited, temp_visited, order)?;
            }
        }

        temp_visited.remove(task_id);
        visited.insert(task_id.to_string());
        order.push(task_id.to_string());

        Ok(())
    }

    fn calculate_overall_progress(&self) -> u8 {
        if self.tasks.is_empty() {
            return 0;
        }

        let total_progress: u32 = self
            .tasks
            .iter()
            .map(|t| t.progress_percentage as u32)
            .sum();
        (total_progress / self.tasks.len() as u32) as u8
    }

    fn resolve_template(&self, template: &str) -> String {
        let mut resolved = template.to_string();

        // Replace variables from execution context
        for (key, value) in &self.execution_context.variables {
            let placeholder = format!("${{{}}}", key);
            let value_str = match value {
                Value::String(s) => s.clone(),
                _ => serde_json::to_string(value).unwrap_or_default(),
            };
            resolved = resolved.replace(&placeholder, &value_str);
        }

        // Replace shared memory references
        for (key, value) in &self.execution_context.shared_memory {
            let placeholder = format!("${{memory.{}}}", key);
            let value_str = match value {
                Value::String(s) => s.clone(),
                _ => serde_json::to_string(value).unwrap_or_default(),
            };
            resolved = resolved.replace(&placeholder, &value_str);
        }

        resolved
    }

    pub fn get_task_status_summary(&self) -> Value {
        json!({
            "total_tasks": self.tasks.len(),
            "completed_tasks": self.completed_tasks.len(),
            "failed_tasks": self.failed_tasks.len(),
            "current_task": self.current_task_id,
            "overall_progress": self.calculate_overall_progress(),
            "tasks": self.tasks.iter().map(|t| json!({
                "id": t.id,
                "name": t.name,
                "status": t.status,
                "progress": t.progress_percentage,
                "priority": t.priority
            })).collect::<Vec<_>>()
        })
    }

    async fn get_execution_context_id(&self) -> Result<i64, AppError> {
        // For now, use the execution_id as a fallback
        // In a real implementation, this would get the actual execution context ID
        Ok(self.execution_id.parse::<i64>().unwrap_or(0))
    }

    async fn get_agent_id(&self) -> Result<i64, AppError> {
        // This would be passed in the context or retrieved from the current execution
        // For now, return a placeholder
        Ok(1) // This should be properly implemented
    }

    async fn get_deployment_id(&self) -> Result<i64, AppError> {
        // This would be passed in the context or retrieved from the current execution
        // For now, return a placeholder
        Ok(1) // This should be properly implemented
    }
}
