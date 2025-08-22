use crate::context::ContextOrchestrator;
use crate::gemini::GeminiClient;
use crate::tools::ToolExecutor;
use crate::template::{AgentTemplates, render_template_with_prompt};

#[derive(Debug, Clone)]
pub enum ResumeContext {
    PlatformFunction(String, Value),
    UserInput(String),
}
use commands::{
    Command, CreateConversationCommand, CreateMemoryCommand, GenerateEmbeddingsCommand,
    UpdateExecutionContextQuery,
};
use common::error::AppError;
use common::state::AppState;
use dto::json::agent_executor::{
    ConversationInsights, ConverseRequest, NextStep, ObjectiveDefinition, StepDecision,
    TaskExecutionResult,
};
use dto::json::agent_responses::{
    ActionsList, ExecutableTask, ExecutionAction, LoopDecision, ParameterGenerationResponse,
    SwitchCaseEvaluation, TaskExecution, TaskExecutionResponse, TaskType, TriggerEvaluation,
    ValidationResponse,
};
use dto::json::{StreamEvent, ToolCall, WorkflowCall};
use models::{
    AgentExecutionState, AiAgentWithFeatures, AiTool, AiToolConfiguration, AiWorkflow,
    ApiToolConfiguration, ConversationContent, ConversationMessageType, ConversationRecord,
    ErrorHandlerNodeConfig, ExecutionContextStatus, ImmediateContext, LLMCallNodeConfig,
    MemoryRecord, PlatformFunctionToolConfiguration, ResponseFormat, SchemaField, SwitchNodeConfig,
    ToolCallNodeConfig, TriggerNodeConfig, UserInputNodeConfig, UserInputRequestState,
    UserInputType, WorkflowEdge, WorkflowExecutionState, WorkflowNode, WorkflowNodeType,
};
use queries::{
    GetExecutionContextQuery, GetLLMConversationHistoryQuery, GetMRUMemoriesQuery,
    GetToolByIdQuery, Query,
};
use serde_json::{Value, json};
use std::collections::HashMap;

const MAX_LOOP_ITERATIONS: usize = 50;

fn calculate_memory_importance(content: &str) -> f64 {
    let mut importance: f64 = 0.5;

    let word_count = content.split_whitespace().count();
    if word_count > 20 {
        importance += 0.1;
    }
    if word_count > 50 {
        importance += 0.1;
    }

    // Increase importance for memories with specific identifiers
    if content.contains("id:") || content.contains("ID:") || content.contains("identifier") {
        importance += 0.15;
    }

    // Increase importance for preference-related content
    if content.contains("prefer") || content.contains("like") || content.contains("want") {
        importance += 0.1;
    }

    // Cap at 0.95 to leave room for manual adjustments
    importance.min(0.95)
}

pub struct AgentExecutor {
    agent: AiAgentWithFeatures,
    app_state: AppState,
    context_id: i64,
    conversations: Vec<ConversationRecord>,
    context_orchestrator: ContextOrchestrator,
    tool_executor: ToolExecutor,
    channel: tokio::sync::mpsc::Sender<StreamEvent>,
    memories: Vec<MemoryRecord>,
    user_request: String,
    current_objective: Option<ObjectiveDefinition>,
    conversation_insights: Option<ConversationInsights>,
    executable_tasks: Vec<ExecutableTask>,
    task_results: HashMap<String, TaskExecutionResult>,
    is_in_planning_mode: bool,
    current_workflow_id: Option<i64>,
    current_workflow_state: Option<HashMap<String, Value>>,
    current_workflow_node_id: Option<String>,
    current_workflow_execution_path: Vec<String>,
}

pub struct AgentExecutorBuilder {
    agent: AiAgentWithFeatures,
    app_state: AppState,
    context_id: i64,
    channel: tokio::sync::mpsc::Sender<StreamEvent>,
}

impl AgentExecutorBuilder {
    pub fn new(
        agent: AiAgentWithFeatures,
        context_id: i64,
        app_state: AppState,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Self {
        Self {
            agent,
            context_id,
            app_state,
            channel,
        }
    }

    pub async fn build(self) -> Result<AgentExecutor, AppError> {
        let tool_executor =
            ToolExecutor::new(self.app_state.clone()).with_channel(self.channel.clone());
        let context_orchestrator =
            ContextOrchestrator::new(self.app_state.clone(), self.agent.clone(), self.context_id);

        let mut executor = AgentExecutor {
            agent: self.agent.clone(),
            app_state: self.app_state.clone(),
            context_id: self.context_id,
            context_orchestrator,
            tool_executor,
            user_request: String::new(),
            channel: self.channel,
            memories: Vec::new(),
            conversations: Vec::new(),
            current_objective: None,
            conversation_insights: None,
            executable_tasks: Vec::new(),
            task_results: HashMap::new(),
            is_in_planning_mode: false,
            current_workflow_id: None,
            current_workflow_state: None,
            current_workflow_node_id: None,
            current_workflow_execution_path: Vec::new(),
        };

        let context = GetExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
            .execute(&self.app_state)
            .await?;

        if context.status == ExecutionContextStatus::WaitingForInput {
            if let Some(state) = context.execution_state {
                executor.restore_from_state(state)?;
            }
        }

        Ok(executor)
    }
}

impl AgentExecutor {
    pub async fn new(
        agent: AiAgentWithFeatures,
        context_id: i64,
        app_state: AppState,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Self, AppError> {
        AgentExecutorBuilder::new(agent, context_id, app_state, channel)
            .build()
            .await
    }

    pub async fn resume_execution(
        &mut self,
        resume_context: ResumeContext,
    ) -> Result<(), AppError> {
        let context_id = self.context_id;
        let deployment_id = self.agent.deployment_id;
        let app_state = self.app_state.clone();

        let immediate_context = self.get_immediate_context().await?;
        self.conversations = immediate_context.conversations;
        self.memories = immediate_context.memories;

        let exec_context = GetExecutionContextQuery::new(context_id, deployment_id)
            .execute(&app_state)
            .await?;

        if let Some(state) = exec_context.execution_state {
            self.restore_from_state(state)?;
        }

        match resume_context {
            ResumeContext::PlatformFunction(execution_id, result) => {
                let conversation = self
                    .create_conversation(
                        ConversationContent::PlatformFunctionResult {
                            execution_id: execution_id.clone(),
                            result: serde_json::to_string(&result)
                                .unwrap_or_else(|_| result.to_string()),
                        },
                        ConversationMessageType::PlatformFunctionResult,
                    )
                    .await?;

                self.conversations.push(conversation.clone());
                let _ = self
                    .channel
                    .send(StreamEvent::ConversationMessage(conversation))
                    .await;

                // If we're in a workflow, update the workflow state
                if let Some(workflow_state) = &mut self.current_workflow_state {
                    // Find the node waiting for this execution_id
                    for (key, value) in workflow_state.clone().iter() {
                        if key.ends_with("_output") {
                            if let Some(stored_exec_id) =
                                value.get("execution_id").and_then(|v| v.as_str())
                            {
                                if stored_exec_id == &execution_id {
                                    // Update this node's output with the actual result
                                    workflow_state.insert(key.clone(), result.clone());
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            ResumeContext::UserInput(input) => {
                self.store_user_message(input.clone()).await?;

                // If we're in a workflow, update the current node's output
                if let Some(workflow_state) = &mut self.current_workflow_state {
                    if let Some(node_id) = &self.current_workflow_node_id {
                        let node_output_key = format!("{}_output", node_id);
                        workflow_state.insert(
                            node_output_key,
                            json!({
                                "value": input,
                                "type": "user_input"
                            }),
                        );
                    }
                }
            }
        }

        // Update status to running
        UpdateExecutionContextQuery::new(context_id, deployment_id)
            .with_status(ExecutionContextStatus::Running)
            .execute(&app_state)
            .await?;

        // Continue execution from where we left off
        self.repl().await
    }

    pub async fn execute_with_streaming(&mut self, message: String) -> Result<(), AppError> {
        let request = ConverseRequest { message };
        self.run(request).await
    }

    pub async fn run(&mut self, request: ConverseRequest) -> Result<(), AppError> {
        // Check current execution context status
        let context = GetExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
            .execute(&self.app_state)
            .await?;

        let is_resuming = self.current_objective.is_some()
            || !self.executable_tasks.is_empty()
            || self.current_workflow_id.is_some()
            || context.status == ExecutionContextStatus::WaitingForInput;

        if is_resuming {
            let user_response = self.store_user_message(request.message.clone()).await?;
            self.conversations.push(user_response);

            UpdateExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
                .with_status(ExecutionContextStatus::Running)
                .execute(&self.app_state)
                .await?;

            self.repl().await?;
        } else {
            self.user_request = request.message.clone();

            let store_future = self.store_user_message(request.message);
            let context_future = self.get_immediate_context();

            let (store_result, context) = tokio::join!(store_future, context_future);
            let user_message = store_result?;
            let context = context?;

            self.conversations = context.conversations;
            self.conversations.push(user_message);
            self.memories = context.memories;

            self.repl().await?;
        }

        Ok(())
    }

    async fn repl(&mut self) -> Result<(), AppError> {
        // Check if we're resuming a workflow execution
        if let Some(workflow_id) = self.current_workflow_id {
            // We're in the middle of a workflow - resume it
            let result = self.resume_workflow_execution().await?;

            // Check if workflow is still pending
            let execution_status =
                if let Some(status) = result.get("execution_status").and_then(|s| s.as_str()) {
                    if status == "pending" {
                        // Store the pending workflow result
                        self.store_conversation(
                            ConversationContent::AssistantTaskExecution {
                                task_execution: json!({
                                    "type": "workflow",
                                    "workflow_id": workflow_id,
                                    "result": result,
                                }),
                                execution_status: "pending".to_string(),
                                blocking_reason: None,
                            },
                            ConversationMessageType::AssistantTaskExecution,
                        )
                        .await?;

                        // Workflow is still pending, return early
                        return Ok(());
                    }
                    status.to_string()
                } else {
                    "completed".to_string()
                };

            // Store the workflow result as a task execution
            self.store_conversation(
                ConversationContent::AssistantTaskExecution {
                    task_execution: json!({
                        "type": "workflow",
                        "workflow_id": workflow_id,
                        "result": result,
                    }),
                    execution_status,
                    blocking_reason: None,
                },
                ConversationMessageType::AssistantTaskExecution,
            )
            .await?;

            // Clear workflow state after completion
            self.current_workflow_id = None;
            self.current_workflow_state = None;
            self.current_workflow_node_id = None;
            self.current_workflow_execution_path = Vec::new();

            // Continue with normal agent flow after workflow completion
        }

        let mut iteration = 0;
        loop {
            iteration += 1;
            if iteration > MAX_LOOP_ITERATIONS {
                self.generate_and_send_summary().await?;
                return Ok(());
            }

            let decision = self.decide_next_step().await?;

            if !self.process_decision(decision).await.unwrap() {
                return Ok(());
            }
        }
    }

    async fn process_decision(&mut self, decision: StepDecision) -> Result<bool, AppError> {
        match decision.next_step {
            NextStep::Acknowledge => {
                if let Some(ack_data) = decision.acknowledgment {
                    self.store_conversation(
                        ConversationContent::AssistantAcknowledgment {
                            acknowledgment_message: ack_data.message,
                            further_action_required: ack_data.further_action_required,
                            reasoning: decision.reasoning.clone(),
                        },
                        ConversationMessageType::AssistantAcknowledgment,
                    )
                    .await?;

                    self.current_objective = Some(ack_data.objective);

                    Ok(ack_data.further_action_required)
                } else {
                    Err(AppError::Internal(
                        "Acknowledgment data missing for acknowledge step".to_string(),
                    ))
                }
            }

            NextStep::GatherContext => {
                // Validate that context_gathering_objective is provided
                let objective = decision.context_gathering_objective.as_deref();
                if objective.is_none() {
                    return Err(AppError::Internal(
                        "Context gathering objective is required when using gather_context step".to_string(),
                    ));
                }
                
                match self.gather_context(objective).await {
                    Ok(_) => Ok(true),
                    Err(e) => Err(e),
                }
            },

            NextStep::DirectExecution => {
                if let Some(action) = decision.direct_execution {
                    let result = self.execute_action(&action).await?;

                    let execution_status = if let Some(status) =
                        result.get("status").and_then(|s| s.as_str())
                    {
                        tracing::info!(
                            "Direct execution result status: {}, result: {:?}",
                            status,
                            result
                        );
                        if status == "pending" {
                            tracing::info!(
                                "Detected pending platform function in direct execution, saving state and pausing"
                            );
                            let execution_state = AgentExecutionState {
                                executable_tasks: self
                                    .executable_tasks
                                    .iter()
                                    .map(|t| serde_json::to_value(t).unwrap())
                                    .collect(),
                                task_results: self
                                    .task_results
                                    .iter()
                                    .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap()))
                                    .collect(),
                                is_in_planning_mode: self.is_in_planning_mode,
                                current_objective: self
                                    .current_objective
                                    .as_ref()
                                    .map(|o| serde_json::to_value(o).unwrap()),
                                conversation_insights: self
                                    .conversation_insights
                                    .as_ref()
                                    .map(|c| serde_json::to_value(c).unwrap()),
                                workflow_state: None,
                                pending_input_request: None,
                            };

                            UpdateExecutionContextQuery::new(
                                self.context_id,
                                self.agent.deployment_id,
                            )
                            .with_execution_state(execution_state)
                            .with_status(ExecutionContextStatus::WaitingForInput)
                            .execute(&self.app_state)
                            .await?;

                            "pending"
                        } else {
                            "completed"
                        }
                    } else {
                        "completed"
                    };

                    let execution = TaskExecution {
                        approach: format!("Direct execution: {}", action.purpose),
                        actions: ActionsList {
                            actions: vec![action.clone()],
                        },
                        expected_result: "Direct execution result".to_string(),
                        actual_result: Some(result.clone()),
                    };

                    self.store_conversation(
                        ConversationContent::AssistantTaskExecution {
                            task_execution: serde_json::to_value(&execution)?,
                            execution_status: execution_status.to_string(),
                            blocking_reason: None,
                        },
                        ConversationMessageType::AssistantTaskExecution,
                    )
                    .await?;

                    if execution_status == "pending" {
                        return Ok(false);
                    }
                }
                Ok(true)
            }

            NextStep::TaskPlanning => {
                self.is_in_planning_mode = true;
                self.store_conversation(
                    ConversationContent::SystemDecision {
                        step: "entering_task_planning".to_string(),
                        reasoning: "Entering task planning mode to build execution plan"
                            .to_string(),
                        confidence: 0.9,
                    },
                    ConversationMessageType::SystemDecision,
                )
                .await?;
                Ok(true)
            }

            NextStep::FinishPlanning => {
                if let Some(tasks) = decision.planned_tasks {
                    self.is_in_planning_mode = false;
                    self.executable_tasks = tasks;

                    let task_summary = json!({
                        "total_tasks": self.executable_tasks.len(),
                        "tasks": self.executable_tasks.clone(),
                    });

                    self.store_conversation(
                        ConversationContent::AssistantActionPlanning {
                            task_execution: task_summary,
                            execution_status: "ready".to_string(),
                            blocking_reason: None,
                        },
                        ConversationMessageType::AssistantActionPlanning,
                    )
                    .await?;
                    Ok(true)
                } else {
                    Err(AppError::Internal(
                        "Finish planning requires planned_tasks".to_string(),
                    ))
                }
            }

            NextStep::ExecuteTasks => {
                if let Err(e) = self.execute_pending_tasks().await {
                    self.store_conversation(
                        ConversationContent::SystemDecision {
                            step: "task_execution_error".to_string(),
                            reasoning: format!("Task execution failed: {e}"),
                            confidence: 1.0,
                        },
                        ConversationMessageType::SystemDecision,
                    )
                    .await?;
                }
                Ok(true)
            }

            NextStep::ValidateProgress => {
                let validation_result = self.validate_execution().await?;
                if validation_result.validation_result.overall_success {
                    self.generate_and_send_summary().await?;
                    Ok(false) // Stop execution
                } else {
                    Ok(true) // Continue
                }
            }

            NextStep::DeliverResponse => {
                self.generate_and_send_summary().await?;
                Ok(false) // Stop execution
            }

            NextStep::RequestUserInput => {
                self.request_user_input().await?;
                Ok(false) // Stop execution
            }

            NextStep::ExamineTool => {
                if let Some(examine_data) = decision.examine_tool {
                    // Find the tool by name
                    let tool = self
                        .agent
                        .tools
                        .iter()
                        .find(|t| t.name == examine_data.tool_name)
                        .ok_or_else(|| {
                            AppError::Internal(format!(
                                "Tool '{}' not found",
                                examine_data.tool_name
                            ))
                        })?;

                    // Store tool examination as context results
                    self.store_conversation(
                        ConversationContent::ContextResults {
                            query: format!("examine_tool: {}", examine_data.tool_name),
                            results: serde_json::to_value(tool)?,
                            result_count: 1,
                            timestamp: chrono::Utc::now(),
                        },
                        ConversationMessageType::ContextResults,
                    )
                    .await?;

                    Ok(true) // Continue execution
                } else {
                    Err(AppError::Internal(
                        "Examine tool data missing for examine_tool step".to_string(),
                    ))
                }
            }

            NextStep::ExamineWorkflow => {
                if let Some(examine_data) = decision.examine_workflow {
                    // Find the workflow by name
                    let workflow = self
                        .agent
                        .workflows
                        .iter()
                        .find(|w| w.name == examine_data.workflow_name)
                        .ok_or_else(|| {
                            AppError::Internal(format!(
                                "Workflow '{}' not found",
                                examine_data.workflow_name
                            ))
                        })?;

                    // Store workflow examination as context results
                    self.store_conversation(
                        ConversationContent::ContextResults {
                            query: format!("examine_workflow: {}", examine_data.workflow_name),
                            results: serde_json::to_value(workflow)?,
                            result_count: 1,
                            timestamp: chrono::Utc::now(),
                        },
                        ConversationMessageType::ContextResults,
                    )
                    .await?;

                    Ok(true) // Continue execution
                } else {
                    Err(AppError::Internal(
                        "Examine workflow data missing for examine_workflow step".to_string(),
                    ))
                }
            }

            NextStep::Complete => {
                // Update status to Idle when execution completes
                UpdateExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
                    .with_status(ExecutionContextStatus::Idle)
                    .execute(&self.app_state)
                    .await?;
                Ok(false) // Stop execution
            }
        }
    }

    async fn decide_next_step(&mut self) -> Result<StepDecision, AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::STEP_DECISION,
            json!({
                "conversation_history": self.get_conversation_history_for_llm(),
                "user_request": self.user_request,
                "current_objective": self.current_objective,
                "conversation_insights": self.conversation_insights,
                "executable_tasks": self.executable_tasks,
                "task_results": self.task_results,
                "available_tools": self.agent.tools.clone(),
                "available_workflows": self.agent.workflows.clone(),
                "available_knowledge_bases": self.agent.knowledge_bases.clone(),
                "iteration_info": {
                    "current_iteration": 1,
                    "max_iterations": MAX_LOOP_ITERATIONS,
                },
                "is_in_planning_mode": self.is_in_planning_mode,
            }),
        )
        .map_err(|e| AppError::Internal(format!("Failed to render step decision template: {e}")))?;

        let decision = self
            .create_main_llm()?
            .generate_structured_content::<StepDecision>(request_body)
            .await?;

        self.store_conversation(
            ConversationContent::SystemDecision {
                step: format!("{:?}", decision.next_step).to_lowercase(),
                reasoning: decision.reasoning.clone(),
                confidence: decision.confidence as f32,
            },
            ConversationMessageType::SystemDecision,
        )
        .await?;

        Ok(decision)
    }

    // Removed - replaced by TaskPlanning/FinishPlanning flow
    // async fn breakdown_tasks(&mut self) -> Result<(), AppError> { ... }

    async fn validate_execution(&mut self) -> Result<ValidationResponse, AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::VALIDATION,
            json!({
                "conversation_history": self.get_conversation_history_for_llm(),
                "user_request": self.user_request,
                "current_objective": self.current_objective,
                "task_results": self.task_results,
                "executable_tasks": self.executable_tasks,
                "available_tools": self.agent.tools.clone(),
                "available_workflows": self.agent.workflows.clone(),
                "available_knowledge_bases": self.agent.knowledge_bases.clone(),
            }),
        )
        .map_err(|e| AppError::Internal(format!("Failed to render validation template: {e}")))?;

        let validation = self
            .create_main_llm()?
            .generate_structured_content::<ValidationResponse>(request_body)
            .await?;

        self.store_conversation(
            ConversationContent::AssistantValidation {
                validation_result: serde_json::to_value(&validation.validation_result)?,
                loop_decision: match validation.loop_decision {
                    LoopDecision::Continue => "continue".to_string(),
                    LoopDecision::Complete => "complete".to_string(),
                    LoopDecision::AbortUnresolvable => "abort_unresolvable".to_string(),
                },
                decision_reasoning: validation.decision_reasoning.clone(),
                next_iteration_focus: validation.next_iteration_focus.clone(),
                has_unresolvable_errors: validation.has_unresolvable_errors,
                unresolvable_error_details: validation.unresolvable_error_details.clone(),
            },
            ConversationMessageType::AssistantValidation,
        )
        .await?;

        Ok(validation)
    }

    async fn generate_and_send_summary(&mut self) -> Result<(), AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::SUMMARY,
            json!({
                "conversation_history": self.get_conversation_history_for_llm(),
                "user_request": self.user_request,
                "task_results": self.task_results,
                "available_tools": self.agent.tools.clone(),
                "available_workflows": self.agent.workflows.clone(),
                "available_knowledge_bases": self.agent.knowledge_bases.clone(),
            }),
        )
        .map_err(|e| AppError::Internal(format!("Failed to render summary template: {e}")))?;

        let summary = self
            .create_main_llm()?
            .generate_structured_content::<Value>(request_body)
            .await?;

        self.store_conversation(
            ConversationContent::AgentResponse {
                response: summary.get("response").unwrap().as_str().unwrap().into(),
                context_used: Default::default(),
            },
            ConversationMessageType::AgentResponse,
        )
        .await?;

        // Update status to Idle after delivering response
        UpdateExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
            .with_status(ExecutionContextStatus::Idle)
            .execute(&self.app_state)
            .await?;

        Ok(())
    }

    async fn gather_context(&mut self, specific_objective: Option<&str>) -> Result<(), AppError> {
        // Create a focused objective for context gathering if provided by step decision
        let context_objective = if let Some(objective) = specific_objective {
            Some(ObjectiveDefinition {
                primary_goal: objective.to_string(),
                success_criteria: vec!["Context gathering completed successfully".to_string()],
                constraints: vec!["Single-purpose search".to_string()],
                context_from_history: "Directed by step decision system".to_string(),
                inferred_intent: objective.to_string(),
            })
        } else {
            self.current_objective.clone()
        };

        let context_results = match self
            .context_orchestrator
            .gather_context(&self.conversations, &context_objective)
            .await
        {
            Ok(results) => results,
            Err(e) => {
                return Err(e);
            }
        };

        println!("{context_results:?}");

        self.store_conversation(
            ConversationContent::ContextResults {
                query: "Context gathering completed".to_string(),
                results: serde_json::to_value(&context_results)?,
                result_count: context_results.len(),
                timestamp: chrono::Utc::now(),
            },
            ConversationMessageType::ContextResults,
        )
        .await?;

        Ok(())
    }

    async fn request_user_input(&mut self) -> Result<(), AppError> {
        let input_request = self.generate_user_input_request().await?;
        let content = self.parse_user_input_request(&input_request)?;

        // Save the current execution state before pausing for input
        self.save_execution_state_for_input(&input_request).await?;

        self.store_conversation(content, ConversationMessageType::UserInputRequest)
            .await?;
        Ok(())
    }

    async fn generate_user_input_request(&self) -> Result<Value, AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::USER_INPUT_REQUEST,
            json!({
                "conversation_history": self.get_conversation_history_for_llm(),
                "current_objective": self.current_objective,
                "working_memory": self.get_working_memory(),
                "available_tools": self.agent.tools.clone(),
                "available_workflows": self.agent.workflows.clone(),
                "available_knowledge_bases": self.agent.knowledge_bases.clone(),
            }),
        )
        .map_err(|e| {
            AppError::Internal(format!("Failed to render user input request template: {e}"))
        })?;

        self.create_main_llm()?
            .generate_structured_content::<serde_json::Value>(request_body)
            .await
    }

    fn parse_user_input_request(
        &self,
        input_request: &Value,
    ) -> Result<ConversationContent, AppError> {
        let question = input_request
            .get("question")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::Internal("Missing question in user input request".to_string())
            })?;

        let context = input_request
            .get("context")
            .and_then(|v| v.as_str())
            .unwrap_or("Additional information needed");

        let input_type = input_request
            .get("input_type")
            .and_then(|v| v.as_str())
            .unwrap_or("text")
            .to_string();
        let options = input_request
            .get("options")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            });
        let default_value = input_request
            .get("default_value")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let placeholder = input_request
            .get("placeholder")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(ConversationContent::UserInputRequest {
            question: question.to_string(),
            context: context.to_string(),
            input_type,
            options,
            default_value,
            placeholder,
        })
    }

    async fn execute_pending_tasks(&mut self) -> Result<(), AppError> {
        if self.executable_tasks.is_empty() {
            return Err(AppError::Internal(
                "No tasks to execute, did we breakdown the tasks first?".to_string(),
            ));
        }

        let task_to_execute = self
            .executable_tasks
            .iter()
            .find(|t| {
                !self.task_results.contains_key(&t.id)
                    && t.dependencies
                        .iter()
                        .all(|dep| self.task_results.contains_key(dep))
            })
            .cloned();

        if let Some(task) = task_to_execute {
            let result = self.execute_single_task(&task).await;

            let task_result = match result {
                Ok(value) => TaskExecutionResult {
                    task_id: task.id.clone(),
                    status: "completed".to_string(),
                    output: Some(value),
                    error: None,
                },
                Err(e) => TaskExecutionResult {
                    task_id: task.id.clone(),
                    status: "failed".to_string(),
                    output: None,
                    error: Some(e.to_string()),
                },
            };

            self.task_results.insert(task.id.clone(), task_result);
        }

        Ok(())
    }

    async fn execute_single_task(&mut self, task: &ExecutableTask) -> Result<Value, AppError> {
        let execution = self.prepare_task_execution(task).await?;

        let result = if execution.can_execute {
            let action_result = self.execute_ready_task(&execution).await?;

            // Check if this was a Platform Function that returned pending
            if let Some(status) = action_result.get("status").and_then(|s| s.as_str()) {
                tracing::info!(
                    "Task execution result status: {}, action_result: {:?}",
                    status,
                    action_result
                );
                if status == "pending" {
                    tracing::info!(
                        "Detected pending platform function, saving state and pausing execution"
                    );
                    // Save execution state before pausing
                    let execution_state = AgentExecutionState {
                        executable_tasks: self
                            .executable_tasks
                            .iter()
                            .map(|t| serde_json::to_value(t).unwrap())
                            .collect(),
                        task_results: self
                            .task_results
                            .iter()
                            .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap()))
                            .collect(),
                        is_in_planning_mode: self.is_in_planning_mode,
                        current_objective: self
                            .current_objective
                            .as_ref()
                            .map(|o| serde_json::to_value(o).unwrap()),
                        conversation_insights: self
                            .conversation_insights
                            .as_ref()
                            .map(|c| serde_json::to_value(c).unwrap()),
                        workflow_state: None,
                        pending_input_request: None,
                    };

                    UpdateExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
                        .with_execution_state(execution_state)
                        .with_status(ExecutionContextStatus::WaitingForInput)
                        .execute(&self.app_state)
                        .await?;

                    // Store the pending result but mark it as pending, not completed
                    self.store_task_execution_result(&execution, &action_result, "pending", None)
                        .await?;

                    // Return early - don't mark task as completed
                    return Ok(action_result);
                }
            }

            action_result
        } else {
            self.create_blocked_result(&execution)
        };

        self.store_task_execution_result(&execution, &result, "completed", None)
            .await?;
        Ok(result)
    }

    async fn prepare_task_execution(
        &self,
        task: &ExecutableTask,
    ) -> Result<TaskExecutionResponse, AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::TASK_EXECUTION,
            json!({
                "task": task,
                "conversation_history": self.get_conversation_history_for_llm(),
                "task_results": self.task_results,
                "available_tools": self.agent.tools.clone(),
                "available_workflows": self.agent.workflows.clone(),
                "available_knowledge_bases": self.agent.knowledge_bases.clone(),
            }),
        )
        .map_err(|e| {
            AppError::Internal(format!("Failed to render task execution template: {e}"))
        })?;

        self.create_main_llm()?
            .generate_structured_content::<TaskExecutionResponse>(request_body)
            .await
    }

    async fn execute_ready_task(
        &mut self,
        execution: &TaskExecutionResponse,
    ) -> Result<Value, AppError> {
        let action = execution
            .task_execution
            .actions
            .actions
            .first()
            .ok_or_else(|| AppError::Internal("No action to execute".to_string()))?;

        match self.execute_action(action).await {
            Ok(value) => Ok(value),
            Err(e) => {
                self.handle_task_execution_error(execution, &e).await?;
                Err(e)
            }
        }
    }

    fn create_blocked_result(&self, execution: &TaskExecutionResponse) -> Value {
        json!({
            "error": "Execution blocked or cannot execute",
            "reason": execution.blocking_reason
        })
    }

    async fn handle_task_execution_error(
        &mut self,
        execution: &TaskExecutionResponse,
        error: &AppError,
    ) -> Result<(), AppError> {
        let error_result = json!({
            "error": error.to_string(),
            "error_type": "execution_failure"
        });

        self.store_task_execution_result(
            execution,
            &error_result,
            "failed",
            Some(error.to_string()),
        )
        .await
    }

    async fn store_task_execution_result(
        &mut self,
        execution: &TaskExecutionResponse,
        result: &Value,
        status: &str,
        blocking_reason: Option<String>,
    ) -> Result<(), AppError> {
        let mut task_execution_with_result = execution.task_execution.clone();
        task_execution_with_result.actual_result = Some(result.clone());

        self.store_conversation(
            ConversationContent::AssistantTaskExecution {
                task_execution: serde_json::to_value(&task_execution_with_result)?,
                execution_status: status.to_string(),
                blocking_reason,
            },
            ConversationMessageType::AssistantTaskExecution,
        )
        .await
    }

    async fn execute_action(&self, action: &ExecutionAction) -> Result<Value, AppError> {
        match &action.action_type {
            TaskType::ToolCall => {
                let tool_call = self.parse_tool_call(&action.details).await?;
                let tool = self
                    .agent
                    .tools
                    .iter()
                    .find(|t| t.name == tool_call.tool_name)
                    .ok_or_else(|| {
                        AppError::BadRequest(format!("Tool '{}' not found", tool_call.tool_name))
                    })?;
                self.tool_executor
                    .execute_tool_immediately(tool, tool_call.parameters)
                    .await
            }
            TaskType::WorkflowCall => {
                let workflow_call = self.parse_workflow_call(&action.details)?;

                let conversation_context: Vec<Value> = self
                    .conversations
                    .iter()
                    .map(|conv| {
                        json!({
                            "id": conv.id,
                            "message_type": conv.message_type,
                            "content": conv.content,
                            "timestamp": conv.timestamp,
                            "type": "conversation"
                        })
                    })
                    .collect();

                let memory_context: Vec<Value> = self
                    .memories
                    .iter()
                    .map(|mem| {
                        json!({
                            "id": mem.id,
                            "content": mem.content,
                            "category": mem.memory_category,
                            "temporal_score": mem.base_temporal_score,
                            "access_count": mem.access_count,
                            "timestamp": mem.last_accessed_at,
                            "type": "memory"
                        })
                    })
                    .collect();

                self.execute_workflow_task(
                    &workflow_call,
                    &self.agent.workflows,
                    &conversation_context,
                    &memory_context,
                    self.channel.clone(),
                )
                .await
            }
        }
    }

    fn schema_fields_to_properties(fields: &[SchemaField]) -> (Value, Vec<String>) {
        let mut properties = serde_json::Map::new();
        let mut required_fields = Vec::new();

        for field in fields {
            let mut field_def = serde_json::Map::new();
            field_def.insert("type".to_string(), json!(field.field_type.to_lowercase()));

            if let Some(desc) = &field.description {
                field_def.insert("description".to_string(), json!(desc));
            }

            if field.required {
                required_fields.push(field.name.clone());
            }

            properties.insert(field.name.clone(), json!(field_def));
        }

        (json!(properties), required_fields)
    }

    fn organize_api_parameters(
        &self,
        flat_params: Value,
        api_config: &ApiToolConfiguration,
    ) -> Result<Value, AppError> {
        let params_obj = flat_params.as_object().ok_or_else(|| {
            AppError::Internal("Generated parameters must be an object".to_string())
        })?;

        let mut url_params = serde_json::Map::new();
        let mut body_params = serde_json::Map::new();

        let field_in_schema = |field_name: &str, schema: &Option<Vec<SchemaField>>| {
            schema
                .as_ref()
                .is_some_and(|fields| fields.iter().any(|f| f.name == field_name))
        };

        for (key, value) in params_obj {
            if field_in_schema(key, &api_config.url_params_schema) {
                url_params.insert(key.clone(), value.clone());
            } else if field_in_schema(key, &api_config.request_body_schema) {
                body_params.insert(key.clone(), value.clone());
            }
        }

        let mut result = serde_json::Map::new();

        if !url_params.is_empty() {
            result.insert("url_params".to_string(), json!(url_params));
        }

        if !body_params.is_empty() {
            result.insert("body".to_string(), json!(body_params));
        }

        Ok(json!(result))
    }

    async fn parse_tool_call(&self, details: &Value) -> Result<ToolCall, AppError> {
        let tool_name = details["tool_name"]
            .as_str()
            .ok_or_else(|| AppError::BadRequest("Tool name not specified".to_string()))?;

        let tool = self.find_tool(tool_name)?;
        let params = self.get_tool_parameters(tool, details).await?;

        Ok(ToolCall {
            tool_name: tool_name.to_string(),
            parameters: params,
        })
    }

    fn find_tool(&self, tool_name: &str) -> Result<&AiTool, AppError> {
        self.agent
            .tools
            .iter()
            .find(|t| t.name == tool_name)
            .ok_or_else(|| AppError::BadRequest(format!("Tool '{tool_name}' not found")))
    }

    async fn get_tool_parameters(&self, tool: &AiTool, details: &Value) -> Result<Value, AppError> {
        if self.tool_needs_llm_params(tool) {
            let generated_params = self.generate_tool_parameters(tool).await?;
            return match &tool.configuration {
                AiToolConfiguration::Api(api_config) => {
                    self.organize_api_parameters(generated_params, api_config)
                }
                _ => Ok(generated_params),
            };
        }

        Ok(self.get_default_tool_parameters(tool, details))
    }

    fn tool_needs_llm_params(&self, tool: &AiTool) -> bool {
        match &tool.configuration {
            AiToolConfiguration::Api(api_config) => {
                api_config.request_body_schema.is_some() || api_config.url_params_schema.is_some()
            }
            AiToolConfiguration::PlatformFunction(func_config) => {
                func_config.input_schema.is_some()
            }
            _ => false,
        }
    }

    fn get_default_tool_parameters(&self, tool: &AiTool, details: &Value) -> Value {
        match &tool.configuration {
            AiToolConfiguration::KnowledgeBase(_) => {
                json!({
                    "query": details.get("query")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&self.user_request)
                })
            }
            AiToolConfiguration::PlatformEvent(event_config) => {
                event_config.event_data.clone().unwrap_or(json!({}))
            }
            _ => json!({}),
        }
    }

    async fn generate_tool_parameters(&self, tool: &AiTool) -> Result<Value, AppError> {
        let parameter_schema = self.build_parameter_schema(tool)?;

        if parameter_schema == json!({}) {
            return Ok(json!({}));
        }

        let response = self
            .request_parameter_generation(tool, &parameter_schema)
            .await?;

        if !response.parameter_generation.can_generate {
            return Err(AppError::BadRequest(format!(
                "Cannot generate parameters for {}: Missing information - {}",
                tool.name,
                response.parameter_generation.missing_information.join(", ")
            )));
        }

        Ok(response.parameter_generation.parameters)
    }

    fn build_parameter_schema(&self, tool: &AiTool) -> Result<Value, AppError> {
        match &tool.configuration {
            AiToolConfiguration::Api(api_config) => self.build_api_schema(api_config),
            AiToolConfiguration::PlatformFunction(func_config) => {
                self.build_platform_function_schema(func_config)
            }
            _ => Err(AppError::Internal(
                "Parameter generation called for non-API/PlatformFunction tool".to_string(),
            )),
        }
    }

    fn build_api_schema(&self, api_config: &ApiToolConfiguration) -> Result<Value, AppError> {
        let mut all_properties = serde_json::Map::new();
        let mut all_required = Vec::new();

        if let Some(schema) = &api_config.request_body_schema {
            let (properties, required) = Self::schema_fields_to_properties(schema);
            if let Some(props) = properties.as_object() {
                all_properties.extend(props.clone());
            }
            all_required.extend(required);
        }

        if let Some(schema) = &api_config.url_params_schema {
            let (properties, required) = Self::schema_fields_to_properties(schema);
            if let Some(props) = properties.as_object() {
                all_properties.extend(props.clone());
            }
            all_required.extend(required);
        }

        if all_properties.is_empty() {
            return Ok(json!({}));
        }

        Ok(json!({
            "type": "OBJECT",
            "properties": all_properties,
            "required": all_required
        }))
    }

    fn build_platform_function_schema(
        &self,
        func_config: &PlatformFunctionToolConfiguration,
    ) -> Result<Value, AppError> {
        let schema = func_config
            .input_schema
            .as_ref()
            .ok_or_else(|| AppError::Internal("No input schema".to_string()))?;

        let (properties, required) = Self::schema_fields_to_properties(schema);

        if properties.as_object().is_none_or(|p| p.is_empty()) {
            return Ok(json!({}));
        }

        Ok(json!({
            "type": "OBJECT",
            "properties": properties,
            "required": required
        }))
    }

    async fn request_parameter_generation(
        &self,
        tool: &AiTool,
        parameter_schema: &Value,
    ) -> Result<ParameterGenerationResponse, AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::PARAMETER_GENERATION,
            json!({
                "conversation_history": self.get_conversation_history_for_llm(),
                "tool_name": tool.name,
                "tool_description": tool.description.as_ref().unwrap_or(&"".to_string()),
                "parameter_schema": parameter_schema,
                "user_request": self.user_request,
                "current_objective": self.current_objective,
                "conversation_insights": self.conversation_insights,
            }),
        )
        .map_err(|e| {
            AppError::Internal(format!(
                "Failed to render parameter generation template: {e}"
            ))
        })?;

        self.create_main_llm()?
            .generate_structured_content::<ParameterGenerationResponse>(request_body)
            .await
    }

    fn parse_workflow_call(&self, details: &Value) -> Result<WorkflowCall, AppError> {
        let workflow_name = details["workflow_name"]
            .as_str()
            .ok_or_else(|| AppError::BadRequest("Workflow name not specified".to_string()))?;

        let inputs = details.get("inputs").cloned().unwrap_or(json!({}));

        Ok(WorkflowCall {
            workflow_name: workflow_name.to_string(),
            inputs,
        })
    }

    async fn store_conversation(
        &mut self,
        content: ConversationContent,
        message_type: ConversationMessageType,
    ) -> Result<(), AppError> {
        let conversation = self.create_conversation(content, message_type).await?;
        self.conversations.push(conversation.clone());

        let _ = self
            .channel
            .send(StreamEvent::ConversationMessage(conversation))
            .await;

        Ok(())
    }

    async fn create_conversation(
        &self,
        content: ConversationContent,
        message_type: ConversationMessageType,
    ) -> Result<ConversationRecord, AppError> {
        let command = CreateConversationCommand::new(
            self.app_state.sf.next_id()? as i64,
            self.context_id,
            content,
            message_type,
        );
        command.execute(&self.app_state).await
    }

    async fn store_user_message(&self, message: String) -> Result<ConversationRecord, AppError> {
        let command = CreateConversationCommand::new(
            self.app_state.sf.next_id()? as i64,
            self.context_id,
            ConversationContent::UserMessage { message },
            ConversationMessageType::UserMessage,
        );
        let conversation = command.execute(&self.app_state).await?;

        let _ = self
            .channel
            .send(StreamEvent::ConversationMessage(conversation.clone()))
            .await;

        Ok(conversation)
    }

    fn get_conversation_history_for_llm(&self) -> Vec<Value> {
        let mut history = Vec::new();
        let mut i = 0;

        while i < self.conversations.len() {
            let conv = &self.conversations[i];

            match conv.message_type {
                ConversationMessageType::ExecutionSummary => {
                    if let ConversationContent::ExecutionSummary {
                        user_message,
                        agent_execution,
                        ..
                    } = &conv.content
                    {
                        history.push(json!({
                            "role": "user",
                            "content": user_message,
                            "timestamp": conv.created_at,
                            "type": "user_message",
                        }));

                        history.push(json!({
                            "role": "model",
                            "content": agent_execution,
                            "timestamp": conv.created_at,
                            "type": "execution_summary",
                        }));

                        i += 2;
                    }
                }
                _ => {
                    history.push(json!({
                        "role": self.map_conversation_type_to_role(&conv.message_type),
                        "content": self.extract_conversation_content(&conv.content),
                        "timestamp": conv.created_at,
                        "type": conv.message_type,
                    }));
                    i += 1;
                }
            }
        }

        history
    }

    fn map_conversation_type_to_role(&self, msg_type: &ConversationMessageType) -> &'static str {
        match msg_type {
            ConversationMessageType::UserMessage => "user",
            _ => "model",
        }
    }

    fn get_working_memory(&self) -> HashMap<String, Value> {
        let mut memory = HashMap::new();

        if !self.executable_tasks.is_empty() {
            memory.insert(
                "pending_tasks".to_string(),
                json!(self.executable_tasks.len()),
            );
            memory.insert(
                "completed_tasks".to_string(),
                json!(self.task_results.len()),
            );
        }

        memory.insert("user_request".to_string(), json!(self.user_request));

        memory.insert(
            "current_iteration".to_string(),
            json!(self.conversations.len()),
        );

        if !self.task_results.is_empty() {
            let successful_tasks = self
                .task_results
                .values()
                .filter(|r| r.status == "completed")
                .count();
            memory.insert("successful_task_count".to_string(), json!(successful_tasks));
        }

        memory
    }

    fn extract_conversation_content(&self, content: &ConversationContent) -> String {
        serde_json::to_string(content).unwrap()
    }

    fn create_main_llm(&self) -> Result<GeminiClient, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_else(|_| "test-key".to_string());
        Ok(GeminiClient::new(
            api_key,
            Some("gemini-2.5-flash".to_string()),
        ))
    }

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

        let result = self
            .execute_workflow(workflow, initial_state, channel)
            .await;

        match &result {
            Ok(res) => {
                tracing::info!("Workflow {} completed successfully", workflow.name);
                tracing::debug!("Workflow result: {:?}", res);
            }
            Err(e) => {
                tracing::error!("Workflow {} failed: {}", workflow.name, e);
            }
        }

        result
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

        // Check if workflow execution was paused for platform function
        if let Some(status) = output.get("status").and_then(|s| s.as_str()) {
            if status == "pending" {
                // Workflow paused, return with pending status
                return Ok(json!({
                    "workflow_id": workflow.id,
                    "workflow_name": workflow.name,
                    "execution_status": "pending",
                    "output": output,
                }));
            }
        }

        Ok(json!({
            "workflow_id": workflow.id,
            "workflow_name": workflow.name,
            "execution_status": "completed",
            "output": output,
        }))
    }

    fn execute_node_recursive<'a>(
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

            // Check if this node returned a pending platform function
            if let Some(status) = output.get("status").and_then(|s| s.as_str()) {
                if status == "pending" {
                    // Don't continue to next nodes - return the pending result
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

    async fn execute_node(
        &self,
        node_type: &WorkflowNodeType,
        all_nodes: &[WorkflowNode],
        all_edges: &[WorkflowEdge],
        workflow_state: &mut HashMap<String, Value>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
        depth: usize,
    ) -> Result<Value, AppError> {
        match node_type {
            WorkflowNodeType::Trigger(config) => {
                self.execute_trigger_node(config, workflow_state).await
            }
            WorkflowNodeType::ErrorHandler(config) => {
                self.execute_error_handler_node(
                    config,
                    all_nodes,
                    all_edges,
                    workflow_state,
                    channel,
                    depth,
                )
                .await
            }
            WorkflowNodeType::LLMCall(config) => self.execute_llm_call_node(config).await,
            WorkflowNodeType::Switch(config) => {
                self.execute_switch_node(config, workflow_state).await
            }
            WorkflowNodeType::ToolCall(config) => {
                self.execute_tool_call_node(config, channel).await
            }
            WorkflowNodeType::UserInput(config) => self.execute_user_input_node(config).await,
        }
    }

    async fn process_next_nodes(
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

    async fn execute_trigger_node(
        &self,
        config: &TriggerNodeConfig,
        workflow_state: &HashMap<String, Value>,
    ) -> Result<Value, AppError> {
        let mut context_summary = json!({
            "inputs": workflow_state.get("inputs").cloned().unwrap_or(json!({})),
            "total_context_items": workflow_state.get("total_context_items").cloned().unwrap_or(json!(0)),
            "has_conversation_history": workflow_state.contains_key("conversation_history"),
            "has_memory_context": workflow_state.contains_key("memory_context"),
        });

        for (key, value) in workflow_state {
            if key.ends_with("_output") {
                context_summary[key] = value.clone();
            }
        }

        let template_context = json!({
            "trigger_condition": config.condition,
            "trigger_description": config.description,
            "workflow_state": context_summary,
        });

        let request_body =
            render_template_with_prompt(AgentTemplates::TRIGGER_EVALUATION, template_context)
                .map_err(|e| {
                    AppError::Internal(format!("Failed to render trigger evaluation template: {e}"))
                })?;

        let evaluation = self
            .create_main_llm()?
            .generate_structured_content::<TriggerEvaluation>(request_body)
            .await?;

        if evaluation.should_trigger {
            Ok(json!({
                "type": "trigger",
                "triggered": true,
                "description": config.description,
                "trigger_condition": config.condition,
                "evaluation": {
                    "reasoning": evaluation.reasoning,
                    "confidence": evaluation.confidence,
                },
                "context": workflow_state,
            }))
        } else {
            Err(AppError::BadRequest(format!(
                "Trigger condition not met: {}. Missing requirements: {}",
                evaluation.reasoning,
                evaluation
                    .missing_requirements
                    .unwrap_or_default()
                    .join(", ")
            )))
        }
    }

    async fn execute_error_handler_node(
        &self,
        config: &ErrorHandlerNodeConfig,
        all_nodes: &[WorkflowNode],
        all_edges: &[WorkflowEdge],
        workflow_state: &mut HashMap<String, Value>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
        depth: usize,
    ) -> Result<Value, AppError> {
        let max_retries = if config.enable_retry {
            config.max_retries
        } else {
            0
        };

        for retry_count in 0..=max_retries {
            let execution_result = self
                .execute_contained_nodes(
                    &config.contained_nodes,
                    all_nodes,
                    all_edges,
                    workflow_state,
                    channel.clone(),
                    depth,
                )
                .await;

            match execution_result {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if config.log_errors {}

                    if retry_count < max_retries {
                        tokio::time::sleep(tokio::time::Duration::from_secs(
                            config.retry_delay_seconds as u64,
                        ))
                        .await;
                        continue;
                    }

                    return Err(e);
                }
            }
        }

        Ok(json!({}))
    }

    async fn execute_contained_nodes(
        &self,
        node_ids: &[String],
        all_nodes: &[WorkflowNode],
        all_edges: &[WorkflowEdge],
        workflow_state: &mut HashMap<String, Value>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
        depth: usize,
    ) -> Result<Value, AppError> {
        let mut last_result = json!({});

        for node_id in node_ids {
            let node = all_nodes
                .iter()
                .find(|n| n.id == *node_id)
                .ok_or_else(|| AppError::Internal(format!("Node {node_id} not found")))?;

            last_result = self
                .execute_node_recursive(
                    node,
                    all_nodes,
                    all_edges,
                    workflow_state.clone(),
                    channel.clone(),
                    depth + 1,
                )
                .await?;
        }

        Ok(last_result)
    }

    async fn execute_llm_call_node(&self, config: &LLMCallNodeConfig) -> Result<Value, AppError> {
        let prompt = config.prompt_template.clone();

        let request_body = serde_json::json!({
            "contents": [{
                "parts": [{"text": prompt}]
            }],
            "generationConfig": {
                "temperature": 0.7,
                "topK": 40,
                "topP": 0.95,
                "maxOutputTokens": 8192,
            }
        }).to_string();

        let llm = self.create_main_llm()?;
        let response: Value = llm
            .generate_structured_content(request_body)
            .await
            .map_err(|e| AppError::Internal(format!("LLM call failed: {e}")))?;

        let response_text = serde_json::to_string(&response)
            .unwrap_or_else(|_| response.to_string());

        match config.response_format {
            ResponseFormat::Text => Ok(json!({
                "type": "llm_response",
                "format": "text",
                "content": response_text,
            })),
            ResponseFormat::Json => serde_json::from_str(&response_text).or_else(|_| {
                Ok(json!({
                    "type": "llm_response",
                    "format": "json",
                    "content": response_text,
                    "parse_error": "Failed to parse response as JSON"
                }))
            }),
        }
    }

    async fn execute_switch_node(
        &self,
        config: &SwitchNodeConfig,
        workflow_state: &HashMap<String, Value>,
    ) -> Result<Value, AppError> {
        let switch_value = json!(config.switch_condition);

        let case_descriptions: Vec<Value> = config
            .cases
            .iter()
            .enumerate()
            .map(|(index, case)| {
                json!({
                    "index": index,
                    "label": case.case_label,
                    "condition": case.case_condition,
                })
            })
            .collect();

        let request_body = render_template_with_prompt(
            AgentTemplates::SWITCH_CASE_EVALUATION,
            json!({
                "switch_value": switch_value,
                "cases": case_descriptions,
                "has_default": config.default_case,
                "workflow_state": workflow_state,
            }),
        )
        .map_err(|e| AppError::Internal(format!("Failed to render switch case template: {e}")))?;

        let evaluation = self
            .create_main_llm()?
            .generate_structured_content::<SwitchCaseEvaluation>(request_body)
            .await?;

        if let Some(case_index) = evaluation.selected_case_index {
            if case_index < config.cases.len() {
                return Ok(json!({
                    "type": "switch",
                    "matched_case": case_index,
                    "case_label": evaluation.selected_case_label,
                    "switch_value": switch_value,
                    "reasoning": evaluation.reasoning,
                    "confidence": evaluation.confidence,
                }));
            }
        }

        if evaluation.use_default && config.default_case {
            Ok(json!({
                "type": "switch",
                "matched_case": "default",
                "switch_value": switch_value,
                "reasoning": evaluation.reasoning,
            }))
        } else {
            Err(AppError::Internal(format!(
                "No matching case for switch value: {}. Reasoning: {}",
                switch_value, evaluation.reasoning
            )))
        }
    }

    async fn execute_tool_call_node(
        &self,
        config: &ToolCallNodeConfig,
        _channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        let tool = GetToolByIdQuery::new(config.tool_id)
            .execute(&self.app_state)
            .await?;

        let parameters = config.input_parameters.clone();

        let result = self
            .tool_executor
            .execute_tool_immediately(&tool, json!(parameters))
            .await?;

        Ok(result)
    }

    async fn execute_user_input_node(
        &self,
        config: &UserInputNodeConfig,
    ) -> Result<Value, AppError> {
        // Send user input request via channel
        {
            let user_input_request = ConversationContent::UserInputRequest {
                question: config.prompt.clone(),
                context: config.description.clone().unwrap_or_default(),
                input_type: match &config.input_type {
                    UserInputType::Text => "text",
                    UserInputType::Number => "number",
                    UserInputType::Select => "select",
                    UserInputType::MultiSelect => "multiselect",
                    UserInputType::Boolean => "boolean",
                    UserInputType::Date => "date",
                }
                .to_string(),
                options: config.options.clone(),
                default_value: config.default_value.clone(),
                placeholder: config.placeholder.clone(),
            };

            let event = StreamEvent::UserInputRequest(user_input_request);
            let _ = self.channel.send(event).await;
        }

        // Return a pending status that will pause the workflow
        Ok(json!({
            "status": "pending",
            "type": "user_input",
            "input_type": match &config.input_type {
                UserInputType::Text => "text",
                UserInputType::Number => "number",
                UserInputType::Select => "select",
                UserInputType::MultiSelect => "multiselect",
                UserInputType::Boolean => "boolean",
                UserInputType::Date => "date",
            },
            "prompt": config.prompt,
            "options": config.options,
            "default_value": config.default_value,
            "placeholder": config.placeholder,
        }))
    }

    pub async fn get_immediate_context(&self) -> Result<ImmediateContext, AppError> {
        let (mru_memories, recent_conversations) =
            tokio::join!(self.get_mru_memories(20), self.get_recent_conversations());

        Ok(ImmediateContext {
            memories: mru_memories?,
            conversations: recent_conversations?,
        })
    }

    async fn get_mru_memories(&self, limit: usize) -> Result<Vec<MemoryRecord>, AppError> {
        GetMRUMemoriesQuery {
            limit: limit as i64,
        }
        .execute(&self.app_state)
        .await
    }

    async fn get_recent_conversations(&self) -> Result<Vec<ConversationRecord>, AppError> {
        let records = GetLLMConversationHistoryQuery {
            context_id: self.context_id,
        }
        .execute(&self.app_state)
        .await?;

        Ok(records)
    }

    pub fn post_execution_processing(mut self) {
        tokio::spawn(async move {
            if let Err(e) = self.check_and_generate_summaries().await {
                tracing::error!("Failed to check and generate summaries: {}", e);
            }
        });
    }

    async fn check_and_generate_summaries(&mut self) -> Result<(), AppError> {
        const TOKEN_THRESHOLD: usize = 100_000; // 100K tokens - trigger threshold
        const TARGET_TOKENS: usize = 80_000; // 80K tokens - target after compression

        let total_uncompressed_tokens: usize = self
            .conversations
            .iter()
            .filter(|conv| !matches!(conv.message_type, ConversationMessageType::ExecutionSummary))
            .map(|conv| conv.token_count as usize)
            .sum();

        tracing::info!(
            "Total uncompressed tokens for context {}: {}",
            self.context_id,
            total_uncompressed_tokens
        );

        // Only generate summaries if we exceed the threshold
        if total_uncompressed_tokens >= TOKEN_THRESHOLD {
            tracing::info!(
                "Token threshold exceeded ({} >= {}), applying sliding window compression",
                total_uncompressed_tokens,
                TOKEN_THRESHOLD
            );

            // Find the start of the current execution (last user message)
            let current_execution_start = self
                .conversations
                .iter()
                .rposition(|msg| matches!(msg.message_type, ConversationMessageType::UserMessage))
                .unwrap_or(self.conversations.len());

            // Calculate how many tokens we need to compress
            let tokens_to_compress = total_uncompressed_tokens - TARGET_TOKENS;

            self.apply_sliding_window_compression(tokens_to_compress, current_execution_start)
                .await?;
        } else {
            tracing::info!(
                "Token threshold not exceeded ({} < {}), skipping compression",
                total_uncompressed_tokens,
                TOKEN_THRESHOLD
            );
        }

        Ok(())
    }

    async fn apply_sliding_window_compression(
        &mut self,
        tokens_to_compress: usize,
        current_execution_start: usize,
    ) -> Result<(), AppError> {
        tracing::info!(
            "Applying sliding window compression: need to compress {} tokens, keeping execution starting at index {}",
            tokens_to_compress,
            current_execution_start
        );

        // Group conversations by execution boundaries
        let mut executions: Vec<(usize, usize, String)> = Vec::new(); // (start_idx, end_idx, user_request)
        let mut current_user_request = String::new();
        let mut execution_start = 0;

        for (idx, conv) in self.conversations.iter().enumerate() {
            // Skip if we're in the current execution
            if idx >= current_execution_start {
                break;
            }

            if matches!(conv.message_type, ConversationMessageType::UserMessage) {
                // If we have a previous execution, save it
                if idx > 0 {
                    executions.push((execution_start, idx, current_user_request.clone()));
                }

                // Start new execution
                execution_start = idx;
                if let ConversationContent::UserMessage { message } = &conv.content {
                    current_user_request = message.clone();
                }
            }
        }

        // Add the last execution before current
        if execution_start < current_execution_start && !current_user_request.is_empty() {
            executions.push((
                execution_start,
                current_execution_start,
                current_user_request,
            ));
        }

        tracing::info!(
            "Found {} past executions to potentially compress",
            executions.len()
        );

        // Process executions from oldest to newest until we've compressed enough tokens
        let mut compressed_tokens = 0;

        for (exec_idx, (start_idx, end_idx, user_request)) in executions.iter().enumerate() {
            // Check if we've compressed enough
            if compressed_tokens >= tokens_to_compress {
                tracing::info!(
                    "Compressed {} tokens (target was {}), stopping",
                    compressed_tokens,
                    tokens_to_compress
                );
                break;
            }

            // Skip if this execution is already summarized
            let already_summarized = self.conversations[*start_idx..*end_idx]
                .iter()
                .any(|msg| matches!(msg.message_type, ConversationMessageType::ExecutionSummary));

            if already_summarized {
                tracing::debug!("Execution {} already has a summary, skipping", exec_idx);
                continue;
            }

            // Calculate tokens in this execution
            let execution_tokens: usize = self.conversations[*start_idx..*end_idx]
                .iter()
                .filter(|conv| {
                    !matches!(conv.message_type, ConversationMessageType::ExecutionSummary)
                })
                .map(|conv| conv.token_count as usize)
                .sum();

            // Skip if this execution has very few tokens
            if execution_tokens < 100 {
                tracing::debug!(
                    "Execution {} has only {} tokens, skipping compression",
                    exec_idx,
                    execution_tokens
                );
                continue;
            }

            // Collect the messages for this execution with error handling
            let execution_messages: Vec<_> = self.conversations[*start_idx..*end_idx]
                .iter()
                .filter_map(|msg| match serde_json::to_value(msg) {
                    Ok(_) => Some(json!({
                        "role": self.map_conversation_type_to_role(&msg.message_type),
                        "content": self.extract_conversation_content(&msg.content),
                    })),
                    Err(e) => {
                        tracing::error!("Failed to serialize conversation message: {}", e);
                        None
                    }
                })
                .collect();

            // Skip if we couldn't serialize any messages
            if execution_messages.is_empty() {
                tracing::error!(
                    "No valid messages found for execution {}, skipping",
                    exec_idx
                );
                continue;
            }

            tracing::info!(
                "Compressing execution {} (messages {}-{}) with {} tokens",
                exec_idx,
                start_idx,
                end_idx - 1,
                execution_tokens
            );

            // Generate summary for this execution
            match self
                .generate_execution_summary_for_messages(user_request.clone(), execution_messages)
                .await
            {
                Ok(summary_tokens) => {
                    compressed_tokens += execution_tokens;

                    // Subtract the summary tokens from compressed amount
                    compressed_tokens = compressed_tokens.saturating_sub(summary_tokens);

                    tracing::info!(
                        "Generated summary with {} tokens (compressed {} tokens)",
                        summary_tokens,
                        execution_tokens - summary_tokens
                    );
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to generate summary for execution {}: {}",
                        exec_idx,
                        e
                    );
                    // Continue with other executions
                }
            }
        }

        // Validate compression effectiveness
        if compressed_tokens == 0 {
            tracing::warn!(
                "Compression failed to reduce any tokens. This may indicate all executions are already compressed or too small."
            );
        } else {
            tracing::info!(
                "Sliding window compression complete. Compressed approximately {} tokens",
                compressed_tokens
            );
        }

        // Recalculate total tokens to verify compression worked
        let new_total_tokens: usize = self
            .conversations
            .iter()
            .filter(|conv| !matches!(conv.message_type, ConversationMessageType::ExecutionSummary))
            .map(|conv| conv.token_count as usize)
            .sum();

        tracing::info!(
            "Token count after compression: {} (target was < {})",
            new_total_tokens,
            100_000 - tokens_to_compress
        );

        Ok(())
    }

    async fn generate_execution_summary_for_messages(
        &mut self,
        user_request: String,
        execution_messages: Vec<serde_json::Value>,
    ) -> Result<usize, AppError> {
        use tiktoken_rs::cl100k_base;

        // Prepare existing memories for context
        let existing_memories: Vec<String> =
            self.memories.iter().map(|m| m.content.clone()).collect();

        let request_body = render_template_with_prompt(
            AgentTemplates::EXECUTION_SUMMARY,
            json!({
                "user_request": user_request,
                "execution_messages": execution_messages,
                "existing_memories": existing_memories,
            }),
        )
        .map_err(|e| {
            AppError::Internal(format!("Failed to render execution summary template: {e}"))
        })?;

        let summary_response = self
            .create_main_llm()?
            .generate_structured_content::<serde_json::Value>(request_body)
            .await
            .map_err(|e| {
                tracing::error!("Failed to generate execution summary: {}", e);
                AppError::Internal(format!("Summary generation failed: {e}"))
            })?;

        let agent_execution = summary_response
            .get("agent_execution")
            .and_then(|v| v.as_str())
            .unwrap_or("Completed the requested task")
            .to_string();

        // Extract working memory items
        let working_memory_items = summary_response
            .get("working_memory")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .map(String::from)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Generate embeddings for all working memory items in batch
        if !working_memory_items.is_empty() {
            match GenerateEmbeddingsCommand::new(working_memory_items.clone())
                .with_task_type("RETRIEVAL_DOCUMENT".to_string())
                .execute(&self.app_state)
                .await
            {
                Ok(embeddings) => {
                    // Verify we got the correct number of embeddings
                    if embeddings.len() != working_memory_items.len() {
                        tracing::error!(
                            "Embedding count mismatch: expected {}, got {}",
                            working_memory_items.len(),
                            embeddings.len()
                        );
                    } else {
                        let mut stored_count = 0;
                        for (memory_content, embedding) in
                            working_memory_items.iter().zip(embeddings.iter())
                        {
                            if embedding.is_empty() {
                                continue;
                            }

                            match self.app_state.sf.next_id() {
                                Ok(id) => {
                                    let memory_id = id as i64;
                                    let importance = calculate_memory_importance(memory_content);

                                    let create_cmd = CreateMemoryCommand {
                                        id: memory_id,
                                        content: memory_content.to_string(),
                                        embedding: embedding.clone(),
                                        memory_category: "working".to_string(),
                                        creation_context_id: Some(self.context_id),
                                        initial_importance: importance,
                                    };

                                    if let Err(e) = create_cmd.execute(&self.app_state).await {
                                        tracing::error!("Failed to store working memory: {}", e);
                                    } else {
                                        stored_count += 1;
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Failed to generate memory ID: {}", e);
                                }
                            }
                        }

                        tracing::info!(
                            "Stored {} out of {} working memory items",
                            stored_count,
                            working_memory_items.len()
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to generate embeddings for working memory: {}", e);
                    // Continue without storing memories - don't fail the entire summary
                }
            };
        }

        // Initialize tokenizer with error handling
        let token_count = match cl100k_base() {
            Ok(bpe) => {
                let full_summary = format!("User: {user_request}\nAgent: {agent_execution}");
                bpe.encode_with_special_tokens(&full_summary).len()
            }
            Err(e) => {
                tracing::error!("Failed to initialize tokenizer for token counting: {}", e);
                // Estimate token count as a fallback (roughly 4 chars per token)
                let full_summary = format!("User: {user_request}\nAgent: {agent_execution}");
                full_summary.len() / 4
            }
        };

        // Store the execution summary
        match self.app_state.sf.next_id() {
            Ok(id) => {
                let command = CreateConversationCommand::new(
                    id as i64,
                    self.context_id,
                    ConversationContent::ExecutionSummary {
                        user_message: user_request,
                        agent_execution,
                        token_count,
                    },
                    ConversationMessageType::ExecutionSummary,
                );

                if let Err(e) = command.execute(&self.app_state).await {
                    tracing::error!("Failed to store execution summary: {}", e);
                    return Err(AppError::Internal(format!("Failed to store summary: {e}")));
                }
            }
            Err(e) => {
                tracing::error!("Failed to generate summary ID: {}", e);
                return Err(AppError::Internal(format!("Failed to generate ID: {e}")));
            }
        }

        Ok(token_count)
    }

    fn restore_from_state(&mut self, state: AgentExecutionState) -> Result<(), AppError> {
        // Restore executable tasks
        self.executable_tasks = state
            .executable_tasks
            .into_iter()
            .filter_map(|v| serde_json::from_value::<ExecutableTask>(v).ok())
            .collect();

        // Restore task results
        self.task_results = state
            .task_results
            .into_iter()
            .filter_map(|(k, v)| {
                serde_json::from_value::<TaskExecutionResult>(v)
                    .ok()
                    .map(|result| (k, result))
            })
            .collect();

        // Restore other state
        self.is_in_planning_mode = state.is_in_planning_mode;

        if let Some(objective) = state.current_objective {
            self.current_objective = serde_json::from_value(objective).ok();
        }

        if let Some(insights) = state.conversation_insights {
            self.conversation_insights = serde_json::from_value(insights).ok();
        }

        // Restore workflow state if we were in the middle of workflow execution
        if let Some(workflow_state) = state.workflow_state {
            self.current_workflow_id = Some(workflow_state.workflow_id);
            self.current_workflow_state = Some(workflow_state.workflow_state);
            self.current_workflow_node_id = Some(workflow_state.current_node_id);
            self.current_workflow_execution_path = workflow_state.execution_path;
        }

        Ok(())
    }

    async fn save_execution_state_for_input(
        &mut self,
        input_request: &Value,
    ) -> Result<(), AppError> {
        // Extract request info from the generated input request
        let question = input_request
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let context = input_request
            .get("context")
            .and_then(|v| v.as_str())
            .unwrap_or("Additional information needed")
            .to_string();

        let input_type = input_request
            .get("input_type")
            .and_then(|v| v.as_str())
            .unwrap_or("text")
            .to_string();

        let options = input_request
            .get("options")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            });

        let default_value = input_request
            .get("default_value")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let placeholder = input_request
            .get("placeholder")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let user_input_state = UserInputRequestState {
            question,
            context,
            input_type,
            options,
            default_value,
            placeholder,
        };

        // Create the execution state
        let execution_state = AgentExecutionState {
            executable_tasks: self
                .executable_tasks
                .iter()
                .map(|t| serde_json::to_value(t).unwrap())
                .collect(),
            task_results: self
                .task_results
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap()))
                .collect(),
            is_in_planning_mode: self.is_in_planning_mode,
            current_objective: self
                .current_objective
                .as_ref()
                .map(|o| serde_json::to_value(o).unwrap()),
            conversation_insights: self
                .conversation_insights
                .as_ref()
                .map(|c| serde_json::to_value(c).unwrap()),
            workflow_state: self.get_current_workflow_state(),
            pending_input_request: Some(user_input_state),
        };

        // Update the execution context with the state
        UpdateExecutionContextQuery::new(self.context_id, self.agent.deployment_id)
            .with_execution_state(execution_state)
            .with_status(ExecutionContextStatus::WaitingForInput)
            .execute(&self.app_state)
            .await?;

        Ok(())
    }

    fn get_current_workflow_state(&self) -> Option<WorkflowExecutionState> {
        match (
            self.current_workflow_id,
            &self.current_workflow_state,
            &self.current_workflow_node_id,
        ) {
            (Some(workflow_id), Some(workflow_state), Some(node_id)) => {
                Some(WorkflowExecutionState {
                    workflow_id,
                    workflow_state: workflow_state.clone(),
                    current_node_id: node_id.clone(),
                    execution_path: self.current_workflow_execution_path.clone(),
                })
            }
            _ => None,
        }
    }

    pub async fn resume_workflow_execution(&mut self) -> Result<Value, AppError> {
        // Get the workflow we were executing
        let workflow_id = self.current_workflow_id.ok_or_else(|| {
            AppError::Internal("No workflow ID found in resume state".to_string())
        })?;

        let workflow = self
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
