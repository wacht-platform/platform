use std::convert;

use super::{
    AgentContext, MemoryEntry, MemoryManager, MemoryQuery, MemoryType, TaskManager, ToolCall,
    ToolResult, WorkflowEngine,
};
use crate::agentic::{MessageParser, xml_parser};
use crate::template::{AgentTemplates, render_template};
use futures::StreamExt;
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared::commands::{
    Command, CreateExecutionMessageCommand, SearchConversationEmbeddingsCommand,
};
use shared::dto::json::StreamEvent;
use shared::error::AppError;
use shared::models::{
    AgentExecutionContextMessage, AiAgentWithFeatures, ExecutionMessageSender, ExecutionMessageType,
};
use shared::queries::{
    GetAiKnowledgeBasesByIdsQuery, GetAiToolsByIdsQuery, GetAiWorkflowsByIdsQuery, Query,
};
use shared::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "response")]
pub struct AcknowledgmentResponse {
    #[serde(rename = "message")]
    pub acknowledgment_message: String,
    pub further_action_required: bool,
    pub reasoning: String,
}

pub struct AgentExecutor {
    pub agent: AiAgentWithFeatures,
    pub app_state: AppState,
    pub task_manager: Option<TaskManager>,
    pub workflow_engine: Option<WorkflowEngine>,
    pub memory_manager: MemoryManager,
    pub message_history: Vec<AgentExecutionContextMessage>,
    pub context_id: i64,
    pub deployment_id: i64,
}

impl AgentExecutor {
    pub async fn new(
        agent: AiAgentWithFeatures,
        deployment_id: i64,
        context_id: i64,
        app_state: AppState,
    ) -> Result<Self, AppError> {
        let memory_manager = MemoryManager::new(app_state.clone(), context_id, deployment_id)?;

        Ok(Self {
            agent,
            app_state,
            context_id,
            deployment_id,
            task_manager: None,
            workflow_engine: None,
            message_history: Vec::new(),
            memory_manager,
        })
    }

    fn extract_title_from_input(&self, input: &str) -> String {
        let title = input.lines().next().unwrap_or(input);
        if title.len() > 50 {
            format!("{}...", &title[..47])
        } else {
            title.to_string()
        }
    }

    fn get_enhanced_system_prompt(&self) -> String {
        let context = json!({
            "agent_name": &self.agent.name,
            "tools": &self.agent.tools,
            "workflows": &self.agent.workflows,
            "knowledge_bases": &self.agent.knowledge_bases
        });

        render_template(AgentTemplates::SYSTEM_PROMPT, &context).unwrap_or_else(|e| {
            tracing::error!("Failed to render system prompt template: {}", e);
            format!("You are {}, an intelligent AI agent.", &self.agent.name)
        })
    }

    async fn load_conversation_history(
        &self,
    ) -> Result<Vec<AgentExecutionContextMessage>, AppError> {
        let execution_context_id = self.context_id;

        Ok(vec![])
    }

    pub async fn execute_with_streaming(
        &mut self,
        user_message: &str,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<(), AppError> {
        self.store_execution_message(
            ExecutionMessageType::UserInput,
            ExecutionMessageSender::User,
            user_message,
            json!({}),
            None,
            None,
        )
        .await?;

        let conversation_history = self.load_conversation_history().await?;
        let memory_query = MemoryQuery {
            query: user_message.to_string(),
            memory_types: vec![
                MemoryType::Episodic,
                MemoryType::Semantic,
                MemoryType::Procedural,
            ],
            max_results: 100,
            min_importance: 0.3,
            time_range: None,
        };
        let relevant_memories = self.memory_manager.search_memories(&memory_query).await?;

        let memories: Vec<MemoryEntry> = relevant_memories.into_iter().map(|m| m.entry).collect();

        let acknowledgment_response = self
            .generate_acknowledgment(
                user_message,
                &conversation_history,
                &memories,
                channel.clone(),
            )
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
        let workflow_engine = WorkflowEngine::new(self.app_state.clone(), Vec::new());

        self.task_manager = Some(task_manager);
        self.workflow_engine = Some(workflow_engine);

        // match self
        //     .task_manager
        //     .as_mut()
        //     .unwrap()
        //     .analyze_and_create_task_plan(user_message, &self.agent_context, &self.app_state)
        //     .await
        // {
        //     Ok(_) => {
        //         let task_summary = self
        //             .task_manager
        //             .as_ref()
        //             .unwrap()
        //             .get_task_status_summary();
        //         let task_count = task_summary
        //             .get("total_tasks")
        //             .and_then(|t| t.as_u64())
        //             .unwrap_or(0);
        //     }
        //     Err(e) => {}
        // }

        // let progress_callback = |message: &str, progress: u8| {};

        // match self
        //     .execute_task_execution_loop(user_message, progress_callback)
        //     .await
        // {
        //     Ok(_) => {
        //         let final_summary = self
        //             .task_manager
        //             .as_ref()
        //             .unwrap()
        //             .get_task_status_summary();
        //         let completed = final_summary
        //             .get("completed_tasks")
        //             .and_then(|c| c.as_u64())
        //             .unwrap_or(0);
        //         let total = final_summary
        //             .get("total_tasks")
        //             .and_then(|t| t.as_u64())
        //             .unwrap_or(0);

        //         self.memory_manager
        //             .store_memory(
        //                 MemoryType::Procedural,
        //                 &format!(
        //                     "Successfully completed agentic task plan for: {}",
        //                     user_message
        //                 ),
        //                 std::collections::HashMap::new(),
        //                 0.7,
        //             )
        //             .await?;
        //     }
        //     Err(e) => {
        //         let summary = self
        //             .task_manager
        //             .as_ref()
        //             .unwrap()
        //             .get_task_status_summary();
        //         let completed = summary
        //             .get("completed_tasks")
        //             .and_then(|c| c.as_u64())
        //             .unwrap_or(0);
        //         let failed = summary
        //             .get("failed_tasks")
        //             .and_then(|f| f.as_u64())
        //             .unwrap_or(0);

        //         if completed > 0 {}
        //         if failed > 0 {}
        //     }
        // }

        // self.store_execution_message(
        //     ExecutionMessageType::AgentResponse,
        //     ExecutionMessageSender::Agent,
        //     "Task execution completed with agentic flow",
        //     json!({}),
        //     None,
        //     None,
        // )
        // .await?;

        // let agent_response =
        //     "Task execution completed successfully with integrated agentic capabilities";
        // self.auto_store_conversation_memory(user_message, agent_response, None)
        //     .await?;

        Ok(())
    }

    async fn generate_acknowledgment(
        &self,
        user_message: &str,
        conversation_history: &[ChatMessage],
        memories: &[MemoryEntry],
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<AcknowledgmentResponse, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.5-flash")
            .max_tokens(4000)
            .temperature(0.3)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build LLM: {}", e)))?;

        let acknowledgment_context = json!({
            "tools": &self.agent.tools,
            "workflows": &self.agent.workflows,
            "knowledge_bases": &self.agent.knowledge_bases,
            "memories": memories
        });

        let system_prompt =
            render_template(AgentTemplates::ACKNOWLEDGMENT, &acknowledgment_context).map_err(
                |e| AppError::Internal(format!("Failed to render acknowledgment template: {}", e)),
            )?;

        let conversation_context =
            self.prepare_conversation_context(conversation_history, user_message, 200_000)?;

        let full_prompt = format!(
            "{}\n\n{}\n\nCurrent request: {}",
            system_prompt, conversation_context, user_message
        );

        let messages = vec![ChatMessage::user().content(&full_prompt).build()];

        let response_text = {
            let mut res = String::new();
            let mut parser = MessageParser::new();
            let mut stream = llm.chat_stream(&messages).await?;

            while let Some(Ok(token)) = stream.next().await {
                res.push_str(&token);

                if let Some(content) = parser.parse(&token) {
                    let _ = channel.send(StreamEvent::Token(content, "".into())).await;
                }
            }

            res
        };

        xml_parser::from_str(&response_text)
    }

    fn prepare_conversation_context(
        &self,
        _conversation_history: &[ChatMessage],
        current_message: &str,
        _max_tokens: usize,
    ) -> Result<String, AppError> {
        // For now, we'll just include the current message
        // TODO: Implement proper conversation history parsing when ChatMessage structure is clarified
        let context = format!("Current Request: {}\n\n", current_message);
        Ok(context)
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

    async fn execute_task_execution_loop(
        &mut self,
        _user_message: &str,
        progress_callback: F,
    ) -> Result<(), AppError> {
        let task_manager = self.task_manager.as_mut().unwrap();

        match task_manager
            .execute_task_plan(&self.agent_context, &self.app_state, progress_callback)
            .await
        {
            Ok(_) => {
                self.validate_agentic_progress_and_adjust_tasks(&[], &[])
                    .await?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    async fn validate_agentic_progress_and_adjust_tasks(
        &mut self,
        completed_tasks: &[String],
        task_results: &[ToolResult],
    ) -> Result<(), AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.5-pro")
            .max_tokens(4000)
            .temperature(0.3)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build LLM: {}", e)))?;

        let task_manager = self.task_manager.as_ref().unwrap();
        let task_summary = task_manager.get_task_status_summary();

        let validation_context = json!({
            "task_summary": serde_json::to_string_pretty(&task_summary).unwrap_or_default(),
            "user_request": "Current execution validation",
            "execution_context": {
                "completed_tasks": completed_tasks,
                "task_results_count": task_results.len()
            },
            "recent_actions": task_results.iter().map(|r| json!({
                "action_type": "task_execution",
                "description": format!("Task result: {:?}", r),
                "status": "completed"
            })).collect::<Vec<_>>()
        });

        let validation_prompt = render_template(AgentTemplates::VALIDATION, &validation_context)
            .unwrap_or_else(|e| {
                tracing::error!("Failed to render validation template: {}", e);
                format!(
                    "Analyze the current progress: {}",
                    serde_json::to_string_pretty(&task_summary).unwrap_or_default()
                )
            });

        let response_text = {
            let user_message = ChatMessage::user().content(&validation_prompt).build();
            let response = llm
                .chat(&[user_message])
                .await
                .map_err(|e| AppError::Internal(format!("Progress validation failed: {}", e)))?;
            response.to_string()
        };

        if response_text.to_lowercase().contains("add")
            && response_text.to_lowercase().contains("task")
        {
            // In a full implementation, this would:
            // 1. Parse the LLM response for specific task recommendations
            // 2. Add new tasks to the task manager
            // 3. Modify existing tasks if needed
            // 4. Update task priorities based on progress

            // For now, we'll just acknowledge the need for adjustments
        }

        // Execute workflows if needed (in separate scope to avoid Send issues)
        self.execute_relevant_workflows(&response_text, completed_tasks, task_results)
            .await?;

        Ok(())
    }

    async fn execute_relevant_workflows(
        &self,
        response_text: &str,
        completed_tasks: &[String],
        task_results: &[ToolResult],
    ) -> Result<(), AppError> {
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
            let workflow_engine = self.workflow_engine.as_ref().unwrap();

            for workflow in workflows_to_execute {
                let input_data = json!({
                    "validation_context": response_text,
                    "completed_tasks": completed_tasks,
                    "task_results_count": task_results.len()
                });

                match workflow_engine
                    .execute_workflow(workflow, input_data, None)
                    .await
                {
                    Ok(execution_result) => {}
                    Err(e) => {}
                }
            }
        }

        Ok(())
    }

    pub async fn store_memory(
        &self,
        content: &str,
        memory_type: MemoryType,
        importance: f32,
    ) -> Result<(), AppError> {
        let mut metadata = std::collections::HashMap::new();

        metadata.insert(
            "deployment_id".to_string(),
            serde_json::Value::Number(self.deployment_id.into()),
        );

        metadata.insert(
            "context_id".to_string(),
            serde_json::Value::Number(self.context_id.into()),
        );

        self.memory_manager
            .store_memory(memory_type, content, metadata, importance)
            .await?;
        Ok(())
    }

    pub async fn auto_store_conversation_memory(
        &self,
        user_input: &str,
        agent_response: &str,
        tool_results: Option<&[ToolResult]>,
    ) -> Result<(), AppError> {
        self.store_memory(
            &format!("User asked: {}", user_input),
            MemoryType::Episodic,
            0.6,
        )
        .await?;

        self.store_memory(
            &format!("Agent responded: {}", agent_response),
            MemoryType::Episodic,
            0.5,
        )
        .await?;

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
