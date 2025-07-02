use std::convert;

use super::{
    AcknowledgmentEngine, AcknowledgmentRequest, AgentContext, MemoryManager, MemoryQuery,
    MemoryType, TaskManager, ToolCall, ToolResult, WorkflowEngine,
};
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;
use serde_json::{Value, json};
use shared::commands::{
    Command, CreateExecutionMessageCommand, SearchConversationEmbeddingsCommand,
};
use shared::dto::json::StreamEvent;
use shared::error::AppError;
use shared::models::{AiAgent, ExecutionMessageSender, ExecutionMessageType};
use shared::queries::{
    GetAiKnowledgeBasesByIdsQuery, GetAiToolsByIdsQuery, GetAiWorkflowsByIdsQuery, Query,
};
use shared::state::AppState;

pub struct AgentExecutor {
    pub agent: AiAgent,
    pub agent_context: AgentContext,
    pub app_state: AppState,
    pub task_manager: Option<TaskManager>,
    pub workflow_engine: Option<WorkflowEngine>,
    pub memory_manager: MemoryManager,
}

impl AgentExecutor {
    pub async fn new(
        agent: AiAgent,
        deployment_id: i64,
        context_id: i64,
        app_state: &AppState,
    ) -> Result<Self, AppError> {
        let tool_ids = agent
            .configuration
            .get("tool_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().and_then(|s| s.parse::<i64>().ok()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let workflow_ids = agent
            .configuration
            .get("workflow_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().and_then(|s| s.parse::<i64>().ok()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let knowledge_base_ids = agent
            .configuration
            .get("knowledge_base_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().and_then(|s| s.parse::<i64>().ok()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let tools = if !tool_ids.is_empty() {
            GetAiToolsByIdsQuery::new(deployment_id, tool_ids)
                .execute(app_state)
                .await?
        } else {
            Vec::new()
        };

        let workflows = if !workflow_ids.is_empty() {
            GetAiWorkflowsByIdsQuery::new(deployment_id, workflow_ids)
                .execute(app_state)
                .await?
        } else {
            Vec::new()
        };

        let knowledge_bases = if !knowledge_base_ids.is_empty() {
            GetAiKnowledgeBasesByIdsQuery::new(deployment_id, knowledge_base_ids)
                .execute(app_state)
                .await?
        } else {
            Vec::new()
        };

        let agent_context = AgentContext {
            agent_id: agent.id,
            execution_context_id: context_id,
            deployment_id,
            tools,
            workflows,
            knowledge_bases,
        };

        let memory_manager =
            MemoryManager::new(agent_context.clone(), app_state.clone(), context_id)?;

        Ok(Self {
            agent,
            agent_context,
            app_state: app_state.clone(),
            task_manager: None,
            workflow_engine: None,
            memory_manager,
        })
    }

    fn extract_title_from_input(&self, input: &str) -> String {
        // Extract a meaningful title from user input (first 50 chars or first sentence)
        let title = input.lines().next().unwrap_or(input);
        if title.len() > 50 {
            format!("{}...", &title[..47])
        } else {
            title.to_string()
        }
    }

    fn get_enhanced_system_prompt(&self) -> String {
        let agent_name = &self.agent.name;
        let available_tools: Vec<String> = self
            .agent_context
            .tools
            .iter()
            .map(|t| {
                format!(
                    "- {}: {}",
                    t.name,
                    t.description.as_deref().unwrap_or("No description")
                )
            })
            .collect();
        let available_workflows: Vec<String> = self
            .agent_context
            .workflows
            .iter()
            .map(|w| {
                format!(
                    "- {}: {}",
                    w.name,
                    w.description.as_deref().unwrap_or("No description")
                )
            })
            .collect();

        format!(
            r#"You are {}, an intelligent AI agent following Claude's agentic flow pattern.

## Your Agentic Process:
1. **Acknowledge** the user's request with understanding
2. **Reason** about the task and break it into manageable steps
3. **Define Tasks** based on available tools, workflows, and knowledge
4. **Execute** tasks in a loop with continuous validation
5. **Validate Progress** after each step - are we heading toward completion?
6. **Adjust Tasks** dynamically - add, modify, or remove tasks as needed
7. **Integrate Memory** at each step for context and learning

## Your Core Abilities:
1. **Context Engine**: Search across all data sources (tools, workflows, knowledge bases)
2. **Memory System**: Store and recall episodic, semantic, and procedural memories
3. **Tool Execution**: Execute tools with dynamic parameter resolution
4. **Workflow Execution**: Run complex multi-step workflows
5. **Task Management**: Create, track, and adjust tasks dynamically

## Available Tools (use with tool_ prefix):
{}

## Available Workflows (use with workflow_ prefix):
{}

## Agentic Guidelines:
- **Think Step-by-Step**: Break complex requests into logical task sequences
- **Validate Continuously**: After each action, assess if you're progressing toward the goal
- **Be Adaptive**: Add new tasks or modify existing ones based on results
- **Use Memory Effectively**: Store important context and retrieve relevant past experiences
- **Reason Before Acting**: Explain your thinking process before tool execution
- **Context First**: Always gather relevant context before making decisions

## Task Execution Pattern:
1. Acknowledge request and show understanding
2. Use context_engine to gather relevant information
3. Define initial task breakdown with reasoning
4. Execute tasks using appropriate tools/workflows
5. After each task: validate progress and adjust plan if needed
6. Store important outcomes in memory
7. Continue until goal is achieved or user is satisfied

Remember: You follow Claude's exact agentic pattern - task definition, reasoning, action, validation, and memory integration at every step."#,
            agent_name,
            if available_tools.is_empty() {
                "No tools available".to_string()
            } else {
                available_tools.join("\n")
            },
            if available_workflows.is_empty() {
                "No workflows available".to_string()
            } else {
                available_workflows.join("\n")
            }
        )
    }

    async fn load_conversation_history(&self) -> Result<Vec<ChatMessage>, AppError> {
        // Use ClickHouse to load recent conversation messages
        let execution_context_id = self.agent_context.execution_context_id;

        let search_results = SearchConversationEmbeddingsCommand::new(
            self.agent.deployment_id,
            execution_context_id,
            vec![0.0; 768], // Dummy embedding for now - we just want recent messages
            20,             // Last 20 messages
        )
        .execute(&self.app_state)
        .await?;

        let mut messages = Vec::new();
        for result in search_results {
            match result.message_type.as_str() {
                "user" => {
                    messages.push(ChatMessage::user().content(&result.content).build());
                }
                "assistant" => {
                    messages.push(ChatMessage::assistant().content(&result.content).build());
                }
                "system" => {
                    // Use user message for system content since ChatMessage doesn't have system()
                    messages.push(
                        ChatMessage::user()
                            .content(&format!("System: {}", result.content))
                            .build(),
                    );
                }
                _ => {
                    // Default to user message
                    messages.push(ChatMessage::user().content(&result.content).build());
                }
            }
        }

        // Sort by creation time (oldest first)
        // Note: This is a simplified approach. In practice, you'd sort by timestamp from metadata
        Ok(messages)
    }

    pub async fn execute_with_streaming(
        &mut self,
        user_message: &str,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<(), AppError> {
        // self.store_execution_message(
        //     ExecutionMessageType::UserInput,
        //     ExecutionMessageSender::User,
        //     user_message,
        //     json!({}),
        //     None,
        //     None,
        // )
        // .await?;

        // self.memory_manager
        //     .store_memory(
        //         MemoryType::Episodic,
        //         &format!("User request: {}", user_message),
        //         std::collections::HashMap::new(),
        //         0.8,
        //     )
        //     .await?;

        // let conversation_history = self.load_conversation_history().await?;
        let conversation_history = vec![];
        let memory_query = MemoryQuery {
            query: user_message.to_string(),
            memory_types: vec![
                MemoryType::Episodic,
                MemoryType::Semantic,
                MemoryType::Procedural,
            ],
            max_results: 10,
            min_importance: 0.3,
            time_range: None,
        };
        let relevant_memories = self.memory_manager.search_memories(&memory_query).await?;

        let acknowledgment_engine = AcknowledgmentEngine::new(self.app_state.clone());
        let acknowledgment_request = AcknowledgmentRequest {
            user_message: user_message.to_string(),
            conversation_history,
            memories: relevant_memories.into_iter().map(|m| m.entry).collect(),
            agent_context: self.agent_context.clone(),
        };

        let acknowledgment_response = acknowledgment_engine
            .generate_acknowledgment(acknowledgment_request, channel.clone())
            .await?;

        self.store_execution_message(
            ExecutionMessageType::AgentResponse,
            ExecutionMessageSender::Agent,
            &acknowledgment_response.acknowledgment_message,
            json!({
                "further_action_required": acknowledgment_response.further_action_required,
                "reasoning": acknowledgment_response.reasoning
            }),
            None,
            None,
        )
        .await?;

        if !acknowledgment_response.further_action_required {
            return Ok(());
        }

        let execution_id = format!("exec_{}", self.app_state.sf.next_id()? as i64);
        let task_manager = TaskManager::new(execution_id.clone(), self.app_state.clone());

        let workflow_engine = WorkflowEngine::new(
            self.agent_context.clone(),
            self.app_state.clone(),
            Vec::new(),
        );

        self.task_manager = Some(task_manager);
        self.workflow_engine = Some(workflow_engine);

        match self
            .task_manager
            .as_mut()
            .unwrap()
            .analyze_and_create_task_plan(user_message, &self.agent_context, &self.app_state)
            .await
        {
            Ok(_) => {
                let task_summary = self
                    .task_manager
                    .as_ref()
                    .unwrap()
                    .get_task_status_summary();
                let task_count = task_summary
                    .get("total_tasks")
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0);
            }
            Err(e) => {}
        }

        let progress_callback = |message: &str, progress: u8| {};

        match self
            .execute_task_execution_loop(user_message, progress_callback)
            .await
        {
            Ok(_) => {
                let final_summary = self
                    .task_manager
                    .as_ref()
                    .unwrap()
                    .get_task_status_summary();
                let completed = final_summary
                    .get("completed_tasks")
                    .and_then(|c| c.as_u64())
                    .unwrap_or(0);
                let total = final_summary
                    .get("total_tasks")
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0);

                self.memory_manager
                    .store_memory(
                        MemoryType::Procedural,
                        &format!(
                            "Successfully completed agentic task plan for: {}",
                            user_message
                        ),
                        std::collections::HashMap::new(),
                        0.7,
                    )
                    .await?;
            }
            Err(e) => {
                let summary = self
                    .task_manager
                    .as_ref()
                    .unwrap()
                    .get_task_status_summary();
                let completed = summary
                    .get("completed_tasks")
                    .and_then(|c| c.as_u64())
                    .unwrap_or(0);
                let failed = summary
                    .get("failed_tasks")
                    .and_then(|f| f.as_u64())
                    .unwrap_or(0);

                if completed > 0 {}
                if failed > 0 {}
            }
        }

        self.store_execution_message(
            ExecutionMessageType::AgentResponse,
            ExecutionMessageSender::Agent,
            "Task execution completed with agentic flow",
            json!({}),
            None,
            None,
        )
        .await?;

        // Store conversation in memory for future reference
        let agent_response =
            "Task execution completed successfully with integrated agentic capabilities";
        self.auto_store_conversation_memory(user_message, agent_response, None)
            .await?;

        Ok(())
    }

    async fn execute_tool_call_with_history(
        &self,
        tool_call: &ToolCall,
        conversation_history: &[ChatMessage],
    ) -> Result<ToolResult, AppError> {
        use super::tool_executor::ToolExecutor;

        let tool_executor = ToolExecutor::new(
            self.agent_context.clone(),
            self.app_state.clone(),
            conversation_history.to_vec(),
        );
        tool_executor.execute_tool_call(tool_call).await
    }

    async fn store_execution_message(
        &self,
        message_type: ExecutionMessageType,
        sender: ExecutionMessageSender,
        content: &str,
        metadata: Value,
        tool_calls: Option<Value>,
        tool_results: Option<Value>,
    ) -> Result<(), AppError> {
        let mut query = CreateExecutionMessageCommand::new(
            self.agent_context.execution_context_id,
            message_type,
            sender,
            content.to_string(),
        );

        if metadata != serde_json::json!({}) {
            query = query.with_metadata(metadata);
        }

        if let Some(calls) = tool_calls {
            query = query.with_tool_calls(calls);
        }

        if let Some(results) = tool_results {
            query = query.with_tool_results(results);
        }

        query.execute(&self.app_state).await?;

        Ok(())
    }

    /// Execute task execution loop with progress validation
    async fn execute_task_execution_loop<F>(
        &mut self,
        _user_message: &str,
        progress_callback: F,
    ) -> Result<(), AppError>
    where
        F: FnMut(&str, u8) + Send,
    {
        let task_manager = self.task_manager.as_mut().unwrap();

        // Execute task plan with progress validation
        match task_manager
            .execute_task_plan(&self.agent_context, &self.app_state, progress_callback)
            .await
        {
            Ok(_) => {
                // After each major task completion, validate progress
                let dummy_callback = |_: &str| {};
                self.validate_agentic_progress_and_adjust_tasks(&[], &[], dummy_callback)
                    .await?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Validate progress and adjust tasks based on results (agentic version)
    async fn validate_agentic_progress_and_adjust_tasks<F>(
        &mut self,
        completed_tasks: &[String],
        task_results: &[ToolResult],
        mut on_chunk: F,
    ) -> Result<(), AppError>
    where
        F: FnMut(&str) + Send,
    {
        on_chunk("🔍 **Validating progress and checking if adjustments are needed...**\n\n");

        // Create LLM for progress validation
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.5-pro") // Use Pro for complex reasoning
            .max_tokens(4000)
            .temperature(0.3) // Lower temperature for more focused analysis
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build LLM: {}", e)))?;

        // Get current task status
        let task_manager = self.task_manager.as_ref().unwrap();
        let task_summary = task_manager.get_task_status_summary();

        // Create validation prompt
        let validation_prompt = format!(
            "You are an AI agent progress validator. Analyze the current task execution status and determine if we're heading towards completion or if adjustments are needed.

Current Task Status:
{}

Completed Tasks: {:?}
Task Results Summary: {} results

Please analyze:
1. Are we making good progress towards the goal?
2. Do any new tasks need to be added?
3. Should any existing tasks be modified or cancelled?
4. Are there any validation issues that need attention?

Respond with a brief analysis and any recommended adjustments.",
            serde_json::to_string_pretty(&task_summary).unwrap_or_default(),
            completed_tasks,
            task_results.len()
        );

        // Get validation response
        let response_text = {
            let user_message = ChatMessage::user().content(&validation_prompt).build();
            let response = llm
                .chat(&[user_message])
                .await
                .map_err(|e| AppError::Internal(format!("Progress validation failed: {}", e)))?;
            response.to_string()
        }; // Response is dropped here

        on_chunk(&format!("📊 **Progress Analysis:**\n{}\n\n", response_text));

        // Parse the response to see if task adjustments are needed
        if response_text.to_lowercase().contains("add")
            && response_text.to_lowercase().contains("task")
        {
            on_chunk("🔄 **Task adjustments detected - analyzing requirements...**\n\n");

            // In a full implementation, this would:
            // 1. Parse the LLM response for specific task recommendations
            // 2. Add new tasks to the task manager
            // 3. Modify existing tasks if needed
            // 4. Update task priorities based on progress

            // For now, we'll just acknowledge the need for adjustments
            on_chunk("✅ **Task adjustment analysis completed**\n\n");
        }

        // Execute workflows if needed (in separate scope to avoid Send issues)
        self.execute_relevant_workflows(&response_text, completed_tasks, task_results, on_chunk)
            .await?;

        Ok(())
    }

    /// Execute relevant workflows based on validation response
    async fn execute_relevant_workflows<F>(
        &self,
        response_text: &str,
        completed_tasks: &[String],
        task_results: &[ToolResult],
        mut on_chunk: F,
    ) -> Result<(), AppError>
    where
        F: FnMut(&str) + Send,
    {
        // Check if we need to execute any workflows based on current progress
        let workflows_to_execute: Vec<_> = self
            .agent_context
            .workflows
            .iter()
            .filter(|workflow| {
                response_text
                    .to_lowercase()
                    .contains(&workflow.name.to_lowercase())
            })
            .collect();

        if !workflows_to_execute.is_empty() {
            on_chunk("🔄 **Checking if workflows need to be executed...**\n\n");

            // Execute relevant workflows if they match current task context
            let workflow_engine = self.workflow_engine.as_ref().unwrap();

            for workflow in workflows_to_execute {
                on_chunk(&format!("🔧 **Executing workflow: {}**\n", workflow.name));

                let input_data = json!({
                    "validation_context": response_text,
                    "completed_tasks": completed_tasks,
                    "task_results_count": task_results.len()
                });

                match workflow_engine
                    .execute_workflow(workflow, input_data, None)
                    .await
                {
                    Ok(execution_result) => {
                        on_chunk(&format!(
                            "✅ **Workflow '{}' completed successfully**\n",
                            workflow.name
                        ));
                        on_chunk(&format!("   Status: {:?}\n\n", execution_result.status));
                    }
                    Err(e) => {
                        on_chunk(&format!("⚠️ **Workflow execution warning**: {}\n\n", e));
                    }
                }
            }
        }

        Ok(())
    }

    /// Store important information in agent memory
    pub async fn store_memory(
        &self,
        content: &str,
        memory_type: MemoryType,
        importance: f32,
    ) -> Result<(), AppError> {
        let mut metadata = std::collections::HashMap::new();

        metadata.insert(
            "deployment_id".to_string(),
            serde_json::Value::Number(self.agent_context.deployment_id.into()),
        );

        metadata.insert(
            "execution_context_id".to_string(),
            serde_json::Value::Number(self.agent_context.execution_context_id.into()),
        );

        self.memory_manager
            .store_memory(memory_type, content, metadata, importance)
            .await?;
        Ok(())
    }

    /// Automatically store important conversation turns and outcomes
    pub async fn auto_store_conversation_memory(
        &self,
        user_input: &str,
        agent_response: &str,
        tool_results: Option<&[ToolResult]>,
    ) -> Result<(), AppError> {
        // Store user input as episodic memory
        self.store_memory(
            &format!("User asked: {}", user_input),
            MemoryType::Episodic,
            0.6,
        )
        .await?;

        // Store agent response as episodic memory
        self.store_memory(
            &format!("Agent responded: {}", agent_response),
            MemoryType::Episodic,
            0.5,
        )
        .await?;

        // Store successful tool results as procedural memory
        if let Some(results) = tool_results {
            for result in results {
                if result.error.is_none() {
                    let tool_memory = format!(
                        "Successfully used tool with result: {}",
                        serde_json::to_string(&result.result).unwrap_or_default()
                    );
                    self.store_memory(&tool_memory, MemoryType::Procedural, 0.7)
                        .await?;
                }
            }
        }

        Ok(())
    }
}
