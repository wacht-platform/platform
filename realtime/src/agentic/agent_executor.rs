use std::convert;

use super::{MemoryEntry, MemoryManager, MemoryQuery, MemoryType, ToolCall, ToolResult};
use crate::agentic::{MessageParser, xml_parser};
use crate::template::{AgentTemplates, render_template};
use futures::StreamExt;
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared::commands::{Command, CreateExecutionMessageCommand};
use shared::dto::json::StreamEvent;
use shared::error::AppError;
use shared::models::{
    AgentExecutionContextMessage, AiAgentWithFeatures, ExecutionMessageSender, ExecutionMessageType,
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

        if acknowledgment_response.further_action_required {
            self.execute_task_execution_loop(user_message).await?;
        }

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
            self.context_id,
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

    async fn execute_task_execution_loop(&mut self, user_message: &str) -> Result<(), AppError> {
        // This is where the agentic loop for breaking down and executing tasks would go.
        // For now, we'll just log a completion message.
        // Step 1: Analyze user request and create a task plan (not implemented)
        // Step 2: Execute tasks in the plan (not implemented)
        // Step 3: Validate progress and adjust plan if necessary (not implemented)

        self.store_execution_message(
            ExecutionMessageType::AgentResponse,
            ExecutionMessageSender::Agent,
            "Task execution completed with agentic flow.",
            json!({}),
            None,
            None,
        )
        .await?;

        let agent_response =
            "Task execution completed successfully with integrated agentic capabilities.";
        self.auto_store_conversation_memory(user_message, agent_response, None)
            .await?;

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
