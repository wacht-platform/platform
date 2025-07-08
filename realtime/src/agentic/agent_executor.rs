use crate::agentic::{
    CitationExtractor, ContextAggregator, DecayManager, MessageParser, ToolExecutor,
    WorkflowExecutor, memory_manager::MemoryManager, xml_parser,
};
use crate::template::{AgentTemplates, render_template};
use chrono::Utc;
use futures::StreamExt;
use llm::LLMProvider;
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::{ChatMessage, ReasoningEffort};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared::commands::{
    Command, CreateAgentDynamicContextCommand, CreateConversationCommand,
    CreateExecutionMessageCommand, GenerateEmbeddingCommand, SearchKnowledgeBaseEmbeddingsCommand,
    UpdateMemoryAccessCommand,
};
use shared::dto::json::{
    ActionExecution, ActionExecutionDetails, ActionExecutionXml, ActionStep, ExecutionPlan,
    PlanningIterationResponse, StreamEvent, Task, TaskCorrection, TaskExploration, TaskStatus,
    TaskVerification, ToolCall, WorkflowCall,
};
use shared::error::AppError;
use shared::models::{
    AgentExecutionContextMessage, AiAgentWithFeatures, ExecutionMessageSender,
    ExecutionMessageType, MemoryEntry, MemoryQuery, MemoryRecordV2, MemorySearchResult, MemoryType,
    ToolResult,
};
use shared::queries::{
    GetExecutionMessagesQuery, Query, SearchAgentDynamicContextQuery, SearchMemoriesQuery,
};
use shared::state::AppState;
use std::collections::HashMap;
use std::sync::Arc;
use std::vec;
use tokio::sync::Mutex;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename = "response")]
pub struct AcknowledgmentResponse {
    #[serde(rename = "message")]
    pub acknowledgment_message: String,
    pub further_action_required: bool,
    pub reasoning: String,
}

#[derive(Clone)]
pub struct AgentResponse<T> {
    pub parsed: T,
    pub xml_content: String,
}

pub struct AgentExecutor {
    pub agent: AiAgentWithFeatures,
    pub app_state: AppState,
    pub context_id: i64,
    pub deployment_id: i64,
    pub current_tasks: Arc<Mutex<Vec<Task>>>,
    pub conversations: Vec<ChatMessage>,
    tool_executor: ToolExecutor,
    workflow_executor: WorkflowExecutor,
    memory_manager: Arc<Mutex<MemoryManager>>,
    decay_manager: DecayManager,
    channel: tokio::sync::mpsc::Sender<StreamEvent>,
    memories: Vec<MemoryRecordV2>,
}

impl AgentExecutor {
    pub async fn new(
        agent: AiAgentWithFeatures,
        deployment_id: i64,
        context_id: i64,
        app_state: AppState,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Self, AppError> {
        let tool_executor = ToolExecutor::new(app_state.clone());
        let workflow_executor = WorkflowExecutor::new(app_state.clone());
        let memory_manager = Arc::new(Mutex::new(MemoryManager::new(
            context_id,
            agent.id,
            deployment_id,
            app_state.clone(),
        )));
        let decay_manager = DecayManager::new(app_state.clone());

        Ok(Self {
            agent,
            app_state,
            context_id,
            deployment_id,
            current_tasks: Arc::new(Mutex::new(Vec::new())),
            tool_executor,
            workflow_executor,
            memory_manager,
            decay_manager,
            channel,
            memories: Vec::new(),
            conversations: Vec::new(),
        })
    }

    pub fn create_strong_llm(
        &self,
        system_prompt: Option<&str>,
    ) -> Result<Box<dyn LLMProvider>, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let mut builder = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.5-pro")
            .max_tokens(8000)
            .reasoning_effort(ReasoningEffort::Medium)
            .temperature(0.3);

        if let Some(system) = system_prompt {
            builder = builder.system(system);
        }

        builder
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build strong LLM: {}", e)))
    }

    pub fn create_weak_llm(
        &self,
        system_prompt: Option<&str>,
    ) -> Result<Box<dyn LLMProvider>, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let mut builder = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.5-flash")
            .reasoning_budget_tokens(0)
            .max_tokens(4000)
            .temperature(0.3);

        if let Some(system) = system_prompt {
            builder = builder.system(system);
        }

        builder
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build weak LLM: {}", e)))
    }

    async fn load_conversation_history(
        &self,
    ) -> Result<Vec<AgentExecutionContextMessage>, AppError> {
        let mut messages = GetExecutionMessagesQuery::new(self.context_id)
            .with_limit(20)
            .execute(&self.app_state)
            .await?;

        messages.reverse();

        Ok(messages)
    }

    pub async fn execute_with_streaming(&mut self, user_message: &str) -> Result<(), AppError> {
        CreateConversationCommand {
            id: self.app_state.sf.next_id()? as i64,
            context_id: self.context_id,
            content: user_message.to_string(),
            message_type: "user_message".to_string(),
        }
        .execute(&self.app_state)
        .await?;

        let immediate_context = self
            .decay_manager
            .get_immediate_context(self.context_id)
            .await?;

        self.memories = immediate_context.memories;
        self.conversations = immediate_context
            .conversations
            .iter()
            .map(|v| {
                if v.message_type == "user_message".to_string() {
                    ChatMessage::user().content(v.content.clone()).build()
                } else {
                    ChatMessage::assistant().content(v.content.clone()).build()
                }
            })
            .collect::<Vec<_>>();

        self.conversations
            .push(ChatMessage::user().content(user_message).build());

        let acknowledgment_response = self.generate_acknowledgment().await?;

        if acknowledgment_response.parsed.further_action_required {
            self.execute_task_execution_loop().await?;
        }

        Ok(())
    }

    pub async fn store_dynamic_context(
        &self,
        content: &str,
        source: Option<String>,
    ) -> Result<(), AppError> {
        let embedding = GenerateEmbeddingCommand::new(content.to_string())
            .execute(&self.app_state)
            .await?;

        CreateAgentDynamicContextCommand {
            id: self.app_state.sf.next_id()? as i64,
            execution_context_id: self.context_id,
            content: content.to_string(),
            source,
            embedding: embedding.into(),
        }
        .execute(&self.app_state)
        .await?;

        Ok(())
    }

    pub async fn store_working_memory(&self, key: String, content: String) -> Result<(), AppError> {
        if let Ok(mut memory_manager) = self.memory_manager.try_lock() {
            memory_manager.store_working_memory(key, content);
        }
        Ok(())
    }

    async fn generate_acknowledgment(
        &mut self,
    ) -> Result<AgentResponse<AcknowledgmentResponse>, AppError> {
        let acknowledgment_context = json!({
            "tools": &self.agent.tools,
            "workflows": &self.agent.workflows,
            "knowledge_bases": &self.agent.knowledge_bases,
        });

        let system_prompt =
            render_template(AgentTemplates::ACKNOWLEDGMENT, &acknowledgment_context).map_err(
                |e| AppError::Internal(format!("Failed to render acknowledgment template: {}", e)),
            )?;

        let llm = self.create_weak_llm(Some(&system_prompt))?;

        let response_text = {
            let mut res = String::new();
            let mut parser = MessageParser::new();
            let mut stream = llm.chat_stream(&self.conversations).await?;

            while let Some(Ok(token)) = stream.next().await {
                res.push_str(&token);

                if let Some(content) = parser.parse(&token) {
                    let _ = self.channel.send(StreamEvent::Token(content)).await;
                }
            }

            res
        };

        let parsed: AcknowledgmentResponse = xml_parser::from_str(&response_text)?;

        self.conversations.push(
            ChatMessage::assistant()
                .content(response_text.clone())
                .build(),
        );

        Ok(AgentResponse {
            parsed,
            xml_content: response_text,
        })
    }

    async fn store_execution_message(
        &self,
        message_type: ExecutionMessageType,
        sender: ExecutionMessageSender,
        content: &str,
        extracted_data: Option<Value>,
    ) -> Result<(), AppError> {
        let embedding = if !content.is_empty() {
            Some(
                GenerateEmbeddingCommand::new(content.to_string())
                    .execute(&self.app_state)
                    .await?,
            )
        } else {
            None
        };

        let mut query = CreateExecutionMessageCommand::new(
            self.context_id,
            message_type,
            sender,
            content.to_string(),
        );

        if let Some(emb) = embedding {
            query = query.with_embedding(emb);
        }

        if let Some(data) = extracted_data {
            query = query.with_extracted_data(data);
        }

        query.execute(&self.app_state).await?;

        Ok(())
    }

    async fn execute_task_execution_loop(&mut self) -> Result<(), AppError> {
        self.create_execution_plan().await?;

        let max_iterations = 200;
        let mut iteration = 0;

        while iteration < max_iterations {
            iteration += 1;

            let next_task = self.get_next_executable_task().await?;

            if let Some(task_id) = next_task {
                match self.execute_task_with_stages(&task_id, &[]).await {
                    Ok(result) => {
                        self.update_task_status(
                            &task_id,
                            TaskStatus::Completed,
                            Some(result.clone()),
                            None,
                        )
                        .await?;

                        if let Some(actions_performed) =
                            result.get("actions_performed").and_then(|a| a.as_array())
                        {
                            if actions_performed.len() > 2 {
                                let pattern = format!(
                                    "Task pattern: '{}' successfully completed with {} actions",
                                    task_id,
                                    actions_performed.len()
                                );
                                self.store_memory_async(pattern, MemoryType::Procedural, 0.7);
                            }
                        }
                    }
                    Err(e) => {
                        self.update_task_status(
                            &task_id,
                            TaskStatus::Failed,
                            None,
                            Some(e.to_string()),
                        )
                        .await?;

                        self.create_recovery_tasks(&task_id, &e.to_string(), self.channel.clone())
                            .await?;
                    }
                }

                if self.are_all_tasks_complete().await? {
                    break;
                }

                self.monitor_and_update_tasks(self.channel.clone()).await?;
            } else {
                let tasks = self.current_tasks.lock().await;
                let blocked_count = tasks
                    .iter()
                    .filter(|t| matches!(t.status, TaskStatus::Blocked))
                    .count();
                let pending_count = tasks
                    .iter()
                    .filter(|t| matches!(t.status, TaskStatus::Pending))
                    .count();

                if blocked_count > 0 && pending_count == 0 {
                    // Log internally but don't send to user
                    tracing::info!("Tasks are blocked. Creating resolution tasks...");
                    drop(tasks);
                    self.create_unblocking_tasks(self.channel.clone()).await?;
                } else if pending_count == 0 {
                    break;
                }
            }
        }

        // Don't send technical execution summary to user
        // The verification stage already sent the user-friendly results

        // Just store completion status internally
        self.store_execution_message(
            ExecutionMessageType::AgentResponse,
            ExecutionMessageSender::Agent,
            "", // Empty message since we don't want to store technical details
            Some(json!({
                "execution_complete": true,
                "iterations": iteration
            })),
        )
        .await?;

        // self.auto_store_conversation_memory(user_message, &summary, None)
        //     .await?;

        // self.learn_from_execution(user_message, &summary).await?;

        Ok(())
    }

    pub fn store_memory_async(&self, content: String, memory_type: MemoryType, importance: f64) {
        let app_state = self.app_state.clone();
        let context_id = Some(self.context_id);

        // Convert old memory type to new category
        let memory_category = match memory_type {
            MemoryType::Procedural => "procedural",
            MemoryType::Semantic => "semantic",
            MemoryType::Episodic => "episodic",
            MemoryType::Working => "working",
        }
        .to_string();

        tokio::spawn(async move {
            // Use the new memory_v2 function
            shared::commands::store_memory_async(
                app_state,
                content,
                memory_category,
                context_id,
                importance,
            );
        });
    }

    pub async fn auto_store_conversation_memory(
        &self,
        _user_input: &str,
        _agent_response: &str,
        tool_results: Option<&[ToolResult]>,
    ) -> Result<(), AppError> {
        if let Some(results) = tool_results {
            for result in results {
                if let Some(error) = &result.error {
                    if !error.contains("network") && !error.contains("timeout") {
                        let error_pattern = format!(
                            "Failed pattern: Tool '{}' failed with: {}",
                            result.tool_call_id, error
                        );
                        self.store_memory_async(error_pattern, MemoryType::Semantic, 0.8);
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn search_memories(
        &self,
        query: &MemoryQuery,
    ) -> Result<Vec<MemorySearchResult>, AppError> {
        let query_embedding = GenerateEmbeddingCommand::new(query.query.clone())
            .execute(&self.app_state)
            .await?;

        let memory_type_filter: Vec<String> = query
            .memory_types
            .iter()
            .map(|mt| mt.as_str().to_string())
            .collect();

        let search_results = SearchMemoriesQuery {
            agent_id: self.agent.id,
            query_embedding,
            limit: query.max_results as i64,
            memory_type_filter,
            min_importance: Some(query.min_importance),
            time_range: query.time_range,
        }
        .execute(&self.app_state)
        .await?;

        let mut results = Vec::new();
        for record in search_results {
            let entry: MemoryEntry = record.clone().into();
            let text_relevance = self.calculate_text_relevance(&entry.content, &query.query);

            let semantic_similarity = (1.0 - (record.score / 2.0)).max(0.0);
            let relevance_score = (text_relevance * 0.3) + (semantic_similarity * 0.7);

            results.push(MemorySearchResult {
                entry,
                relevance_score,
                similarity_score: semantic_similarity,
            });
        }

        results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if !results.is_empty() {
            let memory_ids: Vec<i64> = results.iter().map(|r| r.entry.id).collect();
            let _ = UpdateMemoryAccessCommand::new(memory_ids)
                .execute(&self.app_state)
                .await;
        }

        Ok(results)
    }

    fn calculate_text_relevance(&self, content: &str, query: &str) -> f64 {
        let content_lower = content.to_lowercase();
        let query_lower = query.to_lowercase();

        let mut score = 0.0;

        if content_lower.contains(&query_lower) {
            score += 0.5;
        }

        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let content_words: Vec<&str> = content_lower.split_whitespace().collect();

        let mut matched_words = 0;
        for query_word in &query_words {
            if content_words.iter().any(|cw| cw.contains(query_word)) {
                matched_words += 1;
            }
        }

        if !query_words.is_empty() {
            score += (matched_words as f64 / query_words.len() as f64) * 0.5;
        }

        score.clamp(0.0, 1.0)
    }

    async fn search_knowledge_base_documents(&self, query: &str) -> Result<Vec<Value>, AppError> {
        let query_embedding = GenerateEmbeddingCommand::new(query.to_string())
            .execute(&self.app_state)
            .await?;

        let search_futures: Vec<_> = self
            .agent
            .knowledge_bases
            .iter()
            .map(|kb| {
                let query_embedding = query_embedding.clone();
                let kb_clone = kb.clone();
                let app_state = self.app_state.clone();
                async move {
                    let search_results =
                        SearchKnowledgeBaseEmbeddingsCommand::new(kb_clone.id, query_embedding, 10)
                            .execute(&app_state)
                            .await?;

                    let mut kb_results = Vec::new();
                    for result in search_results {
                        let similarity = (1.0 - (result.score / 2.0)).max(0.0);
                        kb_results.push(json!({
                            "type": "document",
                            "document_id": result.document_id,
                            "content": result.content,
                            "score": result.score,
                            "knowledge_base_id": result.knowledge_base_id,
                            "chunk_index": result.chunk_index,
                            "relevance_score": similarity as f64,
                            "source_knowledge_base": {
                                "id": kb_clone.id,
                                "name": kb_clone.name,
                                "description": kb_clone.description
                            }
                        }));
                    }
                    Ok::<Vec<Value>, AppError>(kb_results)
                }
            })
            .collect();

        let results = futures::future::try_join_all(search_futures).await?;

        let all_results = results.into_iter().flatten().collect();

        Ok(all_results)
    }

    async fn search_dynamic_context_vector(&self, query: &str) -> Result<Vec<Value>, AppError> {
        let query_embedding = GenerateEmbeddingCommand::new(query.to_string())
            .execute(&self.app_state)
            .await?;

        let search_results = SearchAgentDynamicContextQuery {
            execution_context_id: self.context_id,
            query_embedding,
            limit: 10,
        }
        .execute(&self.app_state)
        .await?;

        let mut results = Vec::new();
        for result in search_results {
            let similarity = (1.0 - (result.score / 2.0)).max(0.0);
            results.push(json!({
                "type": "dynamic_context",
                "id": result.id,
                "content": result.content,
                "source": result.source.unwrap_or("dynamic".to_string()),
                "created_at": result.created_at,
                "relevance_score": similarity as f64,
            }));
        }

        Ok(results)
    }

    async fn get_next_executable_task(&self) -> Result<Option<String>, AppError> {
        let tasks = self.current_tasks.lock().await;

        for task in tasks.iter() {
            if matches!(task.status, TaskStatus::Pending) {
                let deps_complete =
                    task.dependencies
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .all(|dep_id| {
                            tasks.iter().any(|t| {
                                t.id == *dep_id && matches!(t.status, TaskStatus::Completed)
                            })
                        });

                if deps_complete {
                    return Ok(Some(task.id.clone()));
                }
            }
        }

        Ok(None)
    }

    async fn extract_tool_parameters(
        &self,
        tool: &shared::models::AiTool,
        user_message: &str,
    ) -> Result<Value, AppError> {
        let messages = GetExecutionMessagesQuery::new(self.context_id)
            .with_limit(10)
            .execute(&self.app_state)
            .await?;

        let mut conversation_context = Vec::new();
        for msg in messages.iter().rev() {
            match msg.sender {
                ExecutionMessageSender::User => {
                    conversation_context.push(format!("User: {}", msg.content));
                }
                ExecutionMessageSender::Agent => {
                    conversation_context.push(format!("Agent: {}", msg.content));
                }
                ExecutionMessageSender::System => {
                    conversation_context.push(format!("System: {}", msg.content));
                }
                ExecutionMessageSender::Tool => {
                    conversation_context.push(format!("Tool: {}", msg.content));
                }
            }
        }

        let dynamic_contexts: Vec<
            shared::models::agent_dynamic_context::AgentDynamicContextSearchResult,
        > = Vec::new();

        let context_info = dynamic_contexts
            .iter()
            .map(|ctx| ctx.content.clone())
            .collect::<Vec<_>>()
            .join("\n");

        let full_context = format!(
            "Conversation History:\n{}\n\nAdditional Context:\n{}",
            conversation_context.join("\n"),
            context_info
        );

        let extraction_context = json!({
            "tool_name": tool.name,
            "tool_description": tool.description,
            "tool_type": match tool.tool_type {
                shared::models::AiToolType::Api => "api",
                shared::models::AiToolType::KnowledgeBase => "knowledge_base",
                shared::models::AiToolType::PlatformEvent => "platform_event",
                shared::models::AiToolType::PlatformFunction => "platform_function",
            },
            "tool_configuration": serde_json::to_string(&tool.configuration).unwrap_or_default(),
            "available_parameters": self.get_tool_parameters(tool),
            "user_message": user_message,
            "context": full_context
        });

        let system_prompt = render_template(
            AgentTemplates::TOOL_PARAMETER_EXTRACTION,
            &extraction_context,
        )
        .map_err(|e| {
            AppError::Internal(format!(
                "Failed to render parameter extraction template: {}",
                e
            ))
        })?;

        // Create LLM with system prompt
        let llm = self.create_weak_llm(Some(&system_prompt))?;

        // Simple user prompt with context
        let user_prompt = format!(
            "Tool: {}\nUser Message: {}\nPlease extract the required parameters for this tool.",
            tool.name, user_message
        );

        let messages = vec![ChatMessage::user().content(&user_prompt).build()];
        let response_text = {
            let response = llm.chat(&messages).await.map_err(|e| {
                AppError::Internal(format!("LLM parameter extraction failed: {}", e))
            })?;
            response.to_string()
        };

        serde_json::from_str(&response_text).map_err(|e| {
            AppError::Internal(format!(
                "Failed to parse extracted parameters: {}. Response: {}",
                e, response_text
            ))
        })
    }

    fn get_tool_parameters(&self, tool: &shared::models::AiTool) -> Vec<Value> {
        use shared::models::{AiToolConfiguration, HttpMethod};

        match &tool.configuration {
            AiToolConfiguration::Api(config) => {
                let mut params = Vec::new();

                if let Some(url_params_schema) = &config.url_params_schema {
                    for param in url_params_schema {
                        params.push(json!({
                            "name": param.name,
                            "type": param.field_type,
                            "description": param.description.as_deref().unwrap_or(""),
                            "required": param.required
                        }));
                    }
                }

                if let Some(query_params_schema) = &config.query_params_schema {
                    for param in query_params_schema {
                        params.push(json!({
                            "name": param.name,
                            "type": param.field_type,
                            "description": param.description.as_deref().unwrap_or(""),
                            "required": param.required
                        }));
                    }
                }

                if matches!(
                    config.method,
                    HttpMethod::POST | HttpMethod::PUT | HttpMethod::PATCH
                ) {
                    params.push(json!({
                        "name": "body",
                        "type": "object",
                        "description": "Request body data",
                        "required": false
                    }));
                }

                params
            }
            AiToolConfiguration::KnowledgeBase(_) => {
                vec![json!({
                    "name": "query",
                    "type": "string",
                    "description": "Search query derived from user intent and tool purpose",
                    "required": true
                })]
            }
            AiToolConfiguration::PlatformEvent(config) => {
                vec![json!({
                    "name": "event_data",
                    "type": "object",
                    "description": format!("Data for {} event", config.event_label),
                    "required": false
                })]
            }
            AiToolConfiguration::PlatformFunction(config) => {
                let mut params = Vec::new();
                if let Some(schema) = &config.input_schema {
                    for field in schema {
                        params.push(json!({
                            "name": field.name,
                            "type": field.field_type,
                            "description": field.description,
                            "required": field.required
                        }));
                    }
                }
                params
            }
        }
    }

    async fn execute_tool_task(
        &self,
        tool_call: &ToolCall,
        memories: &[MemoryEntry],
    ) -> Result<Value, AppError> {
        tracing::info!(
            tool_name = %tool_call.tool_name,
            "Starting tool task execution"
        );
        tracing::debug!(
            tool_name = %tool_call.tool_name,
            initial_parameters = %serde_json::to_string_pretty(&tool_call.parameters).unwrap_or_default(),
            "Tool task initial parameters"
        );

        let tool = self
            .agent
            .tools
            .iter()
            .find(|t| t.name == tool_call.tool_name)
            .ok_or_else(|| AppError::NotFound(format!("Tool {} not found", tool_call.tool_name)))?;

        tracing::debug!(
            tool_id = %tool.id,
            tool_name = %tool.name,
            "Found tool configuration"
        );

        let mut enhanced_tool_call = tool_call.clone();

        let needs_extraction = match &tool.configuration {
            shared::models::AiToolConfiguration::KnowledgeBase(_) => {
                !tool_call.parameters.get("query").is_some()
            }
            shared::models::AiToolConfiguration::Api(config) => {
                config.url_params_schema.as_ref().map_or(false, |schema| {
                    schema
                        .iter()
                        .any(|p| p.required && !tool_call.parameters.get(&p.name).is_some())
                }) || config.query_params_schema.as_ref().map_or(false, |schema| {
                    schema.iter().any(|p| {
                        p.required
                            && !tool_call
                                .parameters
                                .get("query_params")
                                .and_then(|qp| qp.get(&p.name))
                                .is_some()
                    })
                })
            }
            shared::models::AiToolConfiguration::PlatformEvent(_) => false,
            shared::models::AiToolConfiguration::PlatformFunction(config) => {
                if let Some(schema) = &config.input_schema {
                    schema.iter().any(|field| {
                        field.required
                            && !tool_call
                                .parameters
                                .get("inputs")
                                .and_then(|inputs| inputs.get(&field.name))
                                .is_some()
                    })
                } else {
                    false
                }
            }
        };

        if needs_extraction {
            tracing::info!("Extracting additional parameters from conversation context");

            let messages = GetExecutionMessagesQuery::new(self.context_id)
                .with_limit(5)
                .execute(&self.app_state)
                .await?;

            let user_message = messages
                .iter()
                .find(|msg| matches!(msg.sender, ExecutionMessageSender::User))
                .map(|msg| msg.content.as_str())
                .unwrap_or("");

            let extracted_params = self.extract_tool_parameters(tool, user_message).await?;

            tracing::debug!(
                extracted_params = %serde_json::to_string_pretty(&extracted_params).unwrap_or_default(),
                "Extracted parameters"
            );

            if let Some(obj) = enhanced_tool_call.parameters.as_object_mut() {
                if let Some(extracted_obj) = extracted_params.as_object() {
                    for (key, value) in extracted_obj {
                        if !obj.contains_key(key) {
                            obj.insert(key.clone(), value.clone());
                        }
                    }
                }
            } else {
                enhanced_tool_call.parameters = extracted_params;
            }
        }

        tracing::debug!(
            tool_name = %enhanced_tool_call.tool_name,
            final_parameters = %serde_json::to_string_pretty(&enhanced_tool_call.parameters).unwrap_or_default(),
            memory_count = %memories.len(),
            "Executing tool with final parameters"
        );

        let result = self
            .tool_executor
            .execute_tool_task(
                &enhanced_tool_call,
                &self.agent.tools,
                memories,
                self.channel.clone(),
            )
            .await;

        match &result {
            Ok(_res) => tracing::info!(
                tool_name = %tool_call.tool_name,
                "Tool task execution completed successfully"
            ),
            Err(e) => tracing::error!(
                tool_name = %tool_call.tool_name,
                error = %e,
                "Tool task execution failed"
            ),
        }

        result
    }

    async fn execute_workflow_task(
        &self,
        workflow_call: &WorkflowCall,
        memories: &[MemoryEntry],
    ) -> Result<Value, AppError> {
        tracing::info!(
            workflow_name = %workflow_call.workflow_name,
            "Starting workflow task execution"
        );
        tracing::debug!(
            workflow_name = %workflow_call.workflow_name,
            inputs = %serde_json::to_string_pretty(&workflow_call.inputs).unwrap_or_default(),
            memory_count = %memories.len(),
            "Workflow task inputs"
        );

        let result = self
            .workflow_executor
            .execute_workflow_task(
                workflow_call,
                &self.agent.workflows,
                memories,
                self.channel.clone(),
            )
            .await;

        match &result {
            Ok(res) => {
                tracing::info!(
                    workflow_name = %workflow_call.workflow_name,
                    "Workflow task execution completed successfully"
                );
                tracing::debug!(
                    workflow_name = %workflow_call.workflow_name,
                    result = %serde_json::to_string_pretty(res).unwrap_or_default(),
                    "Workflow execution result"
                );
            }
            Err(e) => {
                tracing::error!(
                    workflow_name = %workflow_call.workflow_name,
                    error = %e,
                    "Workflow task execution failed"
                );
            }
        }

        let result = result?;

        self.store_dynamic_context(
            &format!(
                "Workflow execution: {} - Result: {}",
                workflow_call.workflow_name,
                serde_json::to_string(&result)?
            ),
            Some("workflow_execution".to_string()),
        )
        .await?;

        Ok(result)
    }

    async fn execute_task_with_stages(
        &mut self,
        task_id: &str,
        memories: &[MemoryEntry],
    ) -> Result<Value, AppError> {
        self.update_task_status(task_id, TaskStatus::InProgress, None, None)
            .await?;

        let task = {
            let tasks = self.current_tasks.lock().await;
            tasks.iter().find(|t| t.id == task_id).cloned()
        };

        let task = task.ok_or_else(|| AppError::NotFound(format!("Task {} not found", task_id)))?;

        let mut memory_manager = self.memory_manager.lock().await;
        memory_manager.store_working_memory(
            format!("task_{}_context", task_id),
            serde_json::to_string(&task.context).unwrap_or_default(),
        );
        drop(memory_manager);

        let mut action_results = Vec::new();
        let mut error_count = 0;
        const MAX_ERRORS: usize = 3;

        // Stage 1: Exploration
        let exploration_response = self.execute_exploration_stage(&task, memories).await?;

        // Process citations from exploration
        // TODO: Add citation extraction from exploration response

        let exploration = exploration_response;

        // Stage 2: Execute Actions
        for (idx, action_step) in exploration.action_steps.step.iter().enumerate() {
            match self
                .execute_action_stage(action_step, &task, memories)
                .await
            {
                Ok(action_execution) => {
                    let action_result = self
                        .process_action_execution(&action_execution)
                        .await
                        .unwrap();

                    tracing::debug!(
                        task_id = %task.id,
                        step = %idx,
                        action_result = %serde_json::to_string_pretty(&action_result).unwrap_or_default(),
                        "Task execution: Action result captured"
                    );

                    action_results.push(json!({
                        "step": idx,
                        "result": action_result,
                        "message": action_execution.message,
                    }));
                }
                Err(error) => {
                    error_count += 1;

                    // Stage 3: Correction (if error occurred)
                    if error_count <= MAX_ERRORS {
                        let correction = self
                            .execute_correction_stage(
                                action_step,
                                &error,
                                &action_results,
                                &task,
                                memories,
                            )
                            .await?;

                        // Execute alternative approach if suggested
                        if !correction.retry_original {
                            // Execute alternative approach
                            if let Ok(alt_result) = self
                                .execute_action_stage(
                                    &correction.alternative_approach,
                                    &task,
                                    memories,
                                )
                                .await
                            {
                                action_results.push(json!({
                                    "step": idx,
                                    "result": alt_result,
                                    "corrected": true,
                                }));
                            }
                        }
                    } else {
                        return Err(AppError::Internal(format!(
                            "Task {} failed after {} correction attempts",
                            task_id, MAX_ERRORS
                        )));
                    }
                }
            }
        }

        tracing::debug!(
            task_id = %task.id,
            action_results_count = %action_results.len(),
            action_results = %serde_json::to_string_pretty(&action_results).unwrap_or_default(),
            "Task execution: Starting verification stage with action results"
        );

        let verification = self
            .execute_verification_stage(&task, &action_results)
            .await?;

        // Update task completion
        self.update_task_status(
            task_id,
            if verification.objective_met {
                TaskStatus::Completed
            } else {
                TaskStatus::InProgress
            },
            Some(json!({
                "completion_percentage": verification.completion_percentage,
                "results_summary": verification.results_summary,
                "action_results": action_results,
            })),
            None,
        )
        .await?;

        if let Some(memory_updates) = verification.memory_updates {
            // Spawn memory updates as a background task - don't block the response
            let memory_manager = self.memory_manager.clone();
            let task_id_clone = task_id.to_string();

            tokio::spawn(async move {
                let memory_manager = memory_manager.lock().await;
                for memory_update in &memory_updates {
                    let memory_type = match memory_update.memory_type.as_str() {
                        "episodic" => MemoryType::Episodic,
                        "procedural" => MemoryType::Procedural,
                        _ => MemoryType::Semantic,
                    };

                    // Store as contextual memory
                    let mut context = HashMap::new();
                    context.insert("task_id".to_string(), json!(task_id_clone.clone()));
                    context.insert("verification_stage".to_string(), json!(true));

                    if let Err(e) = memory_manager
                        .store_contextual_memory(
                            memory_update.content.clone(),
                            memory_type,
                            memory_update.importance,
                            &context,
                        )
                        .await
                    {
                        tracing::warn!(
                            task_id = %task_id_clone,
                            error = %e,
                            "Failed to store memory update in background"
                        );
                    }
                }
            });
        }

        // Add follow-up tasks if any
        if !verification.follow_up_tasks.is_empty() {
            let mut tasks = self.current_tasks.lock().await;
            for (i, follow_up) in verification.follow_up_tasks.iter().enumerate() {
                let new_task = Task {
                    id: format!("task_{}_{}", task_id, i + 1),
                    description: follow_up.description.clone(),
                    status: TaskStatus::Pending,
                    dependencies: Some(vec![task_id.to_string()]),
                    context: json!({
                        "objective": follow_up.objective,
                        "priority": follow_up.priority,
                        "parent_task": task_id,
                        "type": "planned_execution",
                    }),
                    result: None,
                    error: None,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                };
                tasks.push(new_task);
            }
        }

        Ok(json!({
            "task_id": task_id,
            "objective_met": verification.objective_met,
            "completion_percentage": verification.completion_percentage,
            "results_summary": verification.results_summary,
            "actions_performed": action_results,
        }))
    }

    async fn execute_exploration_stage(
        &mut self,
        task: &Task,
        memories: &[MemoryEntry],
    ) -> Result<TaskExploration, AppError> {
        // Get recent conversation history
        let conversation_history = self.load_conversation_history().await?;
        let recent_history: Vec<_> = conversation_history
            .iter()
            .take(10)
            .map(|msg| {
                json!({
                    "sender": msg.sender,
                    "content": msg.content,
                })
            })
            .collect();

        // Get dynamic context
        let dynamic_context = self
            .search_dynamic_context_vector(&task.description)
            .await?;

        let context = json!({
            "task": {
                "id": task.id,
                "objective": task.context.get("objective").unwrap_or(&json!(task.description)),
                "description": task.description,
                "expected_output": task.context.get("expected_output").unwrap_or(&json!("")),
            },
            "memories": memories,
            "dynamic_context": dynamic_context,
            "conversation_history": recent_history,
            "tools": &self.agent.tools,
            "workflows": &self.agent.workflows,
        });

        let system_prompt =
            render_template(AgentTemplates::TASK_EXPLORATION, &context).map_err(|e| {
                AppError::Internal(format!("Failed to render exploration template: {}", e))
            })?;

        // Create LLM with system prompt
        let llm = self.create_weak_llm(Some(&system_prompt))?;

        // Simple user prompt with just task context
        let user_prompt = format!(
            "Task: {}\nObjective: {}\nPlease explore and analyze this task.",
            task.description,
            task.context
                .get("objective")
                .unwrap_or(&json!(task.description))
        );

        let messages = vec![ChatMessage::user().content(&user_prompt).build()];

        let response_text = {
            let mut res = String::new();
            let mut stream = llm.chat_stream(&messages).await?;

            while let Some(Ok(token)) = stream.next().await {
                res.push_str(&token);
                // Don't stream intermediate exploration messages to user
            }

            res
        };

        // Push only assistant response to conversations (user_prompt is internal, not from actual user)
        self.conversations
            .push(ChatMessage::assistant().content(&response_text).build());

        xml_parser::from_str(&response_text)
    }

    async fn execute_action_stage(
        &mut self,
        action: &ActionStep,
        task: &Task,
        _memories: &[MemoryEntry],
    ) -> Result<ActionExecution, AppError> {
        tracing::info!(
            task_id = %task.id,
            action_type = %action.action_type,
            "Executing action stage for task"
        );
        tracing::debug!(
            task_id = %task.id,
            action_type = %action.action_type,
            action_details = %action.details,
            "Action stage details"
        );

        // Get exploration context from working memory
        let mut memory_manager = self.memory_manager.lock().await;
        let exploration_context = memory_manager
            .get_working_memory(&format!("task_{}_exploration", task.id))
            .unwrap_or_else(|| "No exploration context".to_string());

        // Get previous actions from working memory
        let previous_actions = memory_manager
            .get_working_memory(&format!("task_{}_actions", task.id))
            .and_then(|d| serde_json::from_str::<Vec<Value>>(d.as_str()).ok())
            .unwrap_or_default();
        drop(memory_manager);

        let context = json!({
            "task": {
                "id": task.id,
                "objective": task.context.get("objective").unwrap_or(&json!(task.description)),
            },
            "action": action,
            "exploration_context": exploration_context,
            "previous_actions": previous_actions,
        });

        let system_prompt = render_template(AgentTemplates::TASK_ACTION, &context)
            .map_err(|e| AppError::Internal(format!("Failed to render action template: {}", e)))?;

        // Create LLM with system prompt
        let llm = self.create_weak_llm(Some(&system_prompt))?;

        // Simple user prompt with just action context
        let user_prompt = format!(
            "Task: {}\nAction Type: {}\nAction Details: {}\nPlease execute this action.",
            task.description, action.action_type, action.details
        );

        let messages = vec![ChatMessage::user().content(&user_prompt).build()];

        let response_text = {
            let mut res = String::new();
            let mut stream = llm.chat_stream(&messages).await?;

            while let Some(Ok(token)) = stream.next().await {
                res.push_str(&token);
                // Don't stream intermediate action messages to user
            }

            res
        };

        self.conversations
            .push(ChatMessage::assistant().content(&response_text).build());

        let xml_result: ActionExecutionXml = xml_parser::from_str(&response_text)?;
        let action_execution: ActionExecution = xml_result.into();

        tracing::debug!(
            task_id = %task.id,
            action_execution = ?action_execution,
            "Action stage completed, returning ActionExecution"
        );

        Ok(action_execution)
    }

    async fn execute_correction_stage(
        &mut self,
        failed_action: &ActionStep,
        error: &AppError,
        successful_actions: &[Value],
        task: &Task,
        _memories: &[MemoryEntry],
    ) -> Result<TaskCorrection, AppError> {
        let context = json!({
            "task": {
                "id": task.id,
                "objective": task.context.get("objective").unwrap_or(&json!(task.description)),
            },
            "failed_action": failed_action,
            "error": {
                "type": "execution_error",
                "message": error.to_string(),
                "stack": "",
            },
            "successful_actions": successful_actions,
            "available_tools": &self.agent.tools,
        });

        let system_prompt =
            render_template(AgentTemplates::TASK_CORRECTION, &context).map_err(|e| {
                AppError::Internal(format!("Failed to render correction template: {}", e))
            })?;

        // Create LLM with system prompt
        let llm = self.create_weak_llm(Some(&system_prompt))?;

        // Simple user prompt with error context
        let user_prompt = format!(
            "Task: {}\nFailed Action: {} - {}\nError: {}\nPlease provide a correction for this error.",
            task.description,
            failed_action.action_type,
            failed_action.details,
            error.to_string()
        );

        let messages = vec![ChatMessage::user().content(&user_prompt).build()];

        let response_text = {
            let mut res = String::new();
            let mut stream = llm.chat_stream(&messages).await?;

            while let Some(Ok(token)) = stream.next().await {
                res.push_str(&token);
                // Don't stream intermediate correction messages to user
            }

            res
        };

        self.conversations
            .push(ChatMessage::assistant().content(&response_text).build());

        xml_parser::from_str(&response_text)
    }

    async fn execute_verification_stage(
        &self,
        task: &Task,
        action_results: &[Value],
    ) -> Result<TaskVerification, AppError> {
        // Get original user request
        let mut memory_manager = self.memory_manager.lock().await;
        let original_user_request = memory_manager
            .get_working_memory("original_user_message")
            .unwrap_or_else(|| "User request not found".to_string());
        drop(memory_manager);

        let results: Vec<_> = action_results
            .iter()
            .filter_map(|a| a.get("result"))
            .collect();

        tracing::debug!(
            task_id = %task.id,
            results_count = %results.len(),
            results = %serde_json::to_string_pretty(&results).unwrap_or_default(),
            "Verification stage: Extracted results from action_results"
        );

        let context = json!({
            "task": {
                "id": task.id,
                "objective": task.context.get("objective").unwrap_or(&json!(task.description)),
                "expected_output": task.context.get("expected_output").unwrap_or(&json!("")),
            },
            "actions_performed": action_results,
            "results": results,
            "original_user_request": original_user_request,
        });

        let system_prompt =
            render_template(AgentTemplates::TASK_VERIFICATION, &context).map_err(|e| {
                AppError::Internal(format!("Failed to render verification template: {}", e))
            })?;

        // Create LLM with system prompt
        let llm = self.create_weak_llm(Some(&system_prompt))?;

        // Simple user prompt with verification context
        let user_prompt = format!(
            "Task: {}\nOriginal Request: {}\nResults Count: {}\nPlease verify if the task has been completed successfully.",
            task.description,
            original_user_request,
            results.len()
        );

        let messages = vec![ChatMessage::user().content(&user_prompt).build()];

        let response_text = {
            let mut res = String::new();
            let mut parser = MessageParser::new();
            let mut stream = llm.chat_stream(&messages).await?;

            while let Some(Ok(token)) = stream.next().await {
                res.push_str(&token);

                // Stream the verification message to the user
                if let Some(content) = parser.parse(&token) {
                    let _ = self.channel.send(StreamEvent::Token(content)).await;
                }
            }

            res
        };

        xml_parser::from_str(&response_text)
    }

    async fn process_action_execution(
        &self,
        action_execution: &ActionExecution,
    ) -> Result<Value, AppError> {
        tracing::debug!(
            action_type = ?action_execution.execution,
            message = %action_execution.message,
            "Processing action execution"
        );

        match &action_execution.execution {
            ActionExecutionDetails::ToolCall(tool_call) => {
                tracing::info!(
                    tool_name = %tool_call.tool_name,
                    "Executing tool call"
                );
                tracing::debug!(
                    tool_name = %tool_call.tool_name,
                    parameters = %serde_json::to_string_pretty(&tool_call.parameters).unwrap_or_default(),
                    "Tool call parameters"
                );

                let tc = ToolCall {
                    tool_name: tool_call.tool_name.clone(),
                    parameters: tool_call.parameters.clone(),
                };
                let result = self.execute_tool_task(&tc, &[]).await;

                match &result {
                    Ok(res) => tracing::debug!(
                        tool_name = %tool_call.tool_name,
                        result = %serde_json::to_string_pretty(res).unwrap_or_default(),
                        "Tool call completed successfully"
                    ),
                    Err(e) => tracing::error!(
                        tool_name = %tool_call.tool_name,
                        error = %e,
                        "Tool call failed"
                    ),
                }

                result
            }
            ActionExecutionDetails::WorkflowCall(workflow_call) => {
                tracing::info!(
                    workflow_name = %workflow_call.workflow_name,
                    "Executing workflow call"
                );
                tracing::debug!(
                    workflow_name = %workflow_call.workflow_name,
                    inputs = %serde_json::to_string_pretty(&workflow_call.inputs).unwrap_or_default(),
                    "Workflow call inputs"
                );

                let wc = WorkflowCall {
                    workflow_name: workflow_call.workflow_name.clone(),
                    inputs: workflow_call.inputs.clone(),
                };
                let result = self.execute_workflow_task(&wc, &[]).await;

                match &result {
                    Ok(res) => tracing::debug!(
                        workflow_name = %workflow_call.workflow_name,
                        result = %serde_json::to_string_pretty(res).unwrap_or_default(),
                        "Workflow call completed successfully"
                    ),
                    Err(e) => tracing::error!(
                        workflow_name = %workflow_call.workflow_name,
                        error = %e,
                        "Workflow call failed"
                    ),
                }

                result
            }
            ActionExecutionDetails::ContextSearch(context_search) => {
                tracing::info!(
                    query = %context_search.query,
                    "Executing context search"
                );
                // Handle context search
                Ok(json!({
                    "type": "context_search",
                    "query": context_search.query,
                }))
            }
            ActionExecutionDetails::MemoryOperation(memory_op) => {
                tracing::info!(
                    operation_type = %memory_op.operation_type,
                    memory_type = %memory_op.memory_type,
                    "Executing memory operation"
                );
                tracing::debug!(
                    operation_type = %memory_op.operation_type,
                    memory_type = %memory_op.memory_type,
                    content = %memory_op.content,
                    "Memory operation details"
                );
                // Handle memory operation
                Ok(json!({
                    "type": "memory_operation",
                    "operation": memory_op.operation_type,
                }))
            }
        }
    }

    async fn update_task_status(
        &self,
        task_id: &str,
        status: TaskStatus,
        result: Option<Value>,
        error: Option<String>,
    ) -> Result<(), AppError> {
        let mut tasks = self.current_tasks.lock().await;

        if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
            task.status = status;
            task.updated_at = Utc::now();

            if let Some(res) = result {
                task.result = Some(res);
            }

            if let Some(err) = error {
                task.error = Some(err);
            }
        }

        Ok(())
    }

    async fn are_all_tasks_complete(&self) -> Result<bool, AppError> {
        let tasks = self.current_tasks.lock().await;

        Ok(tasks
            .iter()
            .all(|t| matches!(t.status, TaskStatus::Completed | TaskStatus::Failed)))
    }

    async fn monitor_and_update_tasks(
        &self,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<(), AppError> {
        let tasks = self.current_tasks.lock().await;

        let total = tasks.len();
        let completed = tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Completed))
            .count();
        let failed = tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Failed))
            .count();
        let in_progress = tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::InProgress))
            .count();

        // Log progress internally but don't send to user
        tracing::info!(
            "Progress: {}/{} completed, {} failed, {} in progress",
            completed,
            total,
            failed,
            in_progress
        );

        Ok(())
    }

    async fn create_recovery_tasks(
        &self,
        failed_task_id: &str,
        error: &str,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<(), AppError> {
        // Check if the task has a successful result despite the error
        let tasks = self.current_tasks.lock().await;
        let failed_task = tasks.iter().find(|t| t.id == failed_task_id);

        // If the task has a successful result, don't create recovery
        if let Some(task) = failed_task {
            if task.result.is_some() {
                tracing::info!(
                    task_id = %failed_task_id,
                    "Task has result despite error, skipping recovery"
                );
                return Ok(());
            }
        }
        drop(tasks);

        // Check if this is a non-critical error (like memory evaluation)
        if error.contains("memory evaluation") || error.contains("Failed to parse memory") {
            tracing::info!(
                task_id = %failed_task_id,
                error = %error,
                "Non-critical error in post-processing, skipping recovery"
            );
            return Ok(());
        }

        // Log error internally but don't send to user
        tracing::warn!(
            "Task {} failed: {}. Creating recovery strategy...",
            failed_task_id,
            error
        );

        // Create a recovery task
        let recovery_task = Task {
            id: format!("{}_recovery", failed_task_id),
            description: format!("Recover from error in task {}: {}", failed_task_id, error),
            status: TaskStatus::Pending,
            dependencies: Some(vec![]),
            context: json!({
                "original_task_id": failed_task_id,
                "error": error,
                "action": "recovery"
            }),
            result: None,
            error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let mut tasks = self.current_tasks.lock().await;
        tasks.push(recovery_task);

        Ok(())
    }

    async fn create_unblocking_tasks(
        &self,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<(), AppError> {
        let tasks = self.current_tasks.lock().await;
        let blocked_tasks: Vec<_> = tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Blocked))
            .cloned()
            .collect();
        drop(tasks);

        for blocked_task in blocked_tasks {
            let unblock_task = Task {
                id: format!("{}_unblock", blocked_task.id),
                description: format!(
                    "Resolve blocking issue for task: {}",
                    blocked_task.description
                ),
                status: TaskStatus::Pending,
                dependencies: Some(vec![]),
                context: json!({
                    "blocked_task_id": blocked_task.id,
                    "action": "unblock"
                }),
                result: None,
                error: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };

            let mut tasks = self.current_tasks.lock().await;
            tasks.push(unblock_task);
        }

        Ok(())
    }

    async fn generate_execution_summary(&self) -> Result<String, AppError> {
        let tasks = self.current_tasks.lock().await;

        let total = tasks.len();
        let completed = tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Completed))
            .count();
        let failed = tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Failed))
            .count();

        let mut summary = format!(
            "Task execution completed. {} of {} tasks completed successfully.",
            completed, total
        );

        if failed > 0 {
            summary.push_str(&format!(" {} tasks failed.", failed));
        }

        // Add key results
        let key_results: Vec<String> = tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Completed) && t.result.is_some())
            .take(3)
            .map(|t| format!("- {}: Completed", t.description))
            .collect();

        if !key_results.is_empty() {
            summary.push_str("\n\nKey accomplishments:\n");
            summary.push_str(&key_results.join("\n"));
        }

        Ok(summary)
    }

    async fn learn_from_execution(
        &self,
        _user_message: &str,
        _summary: &str,
    ) -> Result<(), AppError> {
        let tasks = self.current_tasks.lock().await;

        let mut tool_combinations = Vec::new();
        let mut failed_approaches = Vec::new();

        for task in tasks.iter() {
            match &task.status {
                TaskStatus::Failed => {
                    if let Some(error) = task.context.get("error") {
                        let insight = format!(
                            "Failed approach: Task '{}' failed - {}",
                            task.description,
                            error.as_str().unwrap_or("unknown error")
                        );
                        failed_approaches.push(insight);
                    }
                }
                TaskStatus::Completed => {
                    if let Some(tool_call) = task.context.get("tool_call") {
                        if let Some(tool_name) = tool_call.get("tool_name").and_then(|v| v.as_str())
                        {
                            tool_combinations.push(tool_name.to_string());
                        }
                    }
                }
                _ => {}
            }
        }

        if tool_combinations.len() > 2 {
            let pattern = format!(
                "Learned pattern: Complex request handled by combining tools: {}",
                tool_combinations.join(" -> ")
            );
            self.store_memory_async(pattern, MemoryType::Procedural, 0.8);
        }

        for failure in failed_approaches {
            self.store_memory_async(failure, MemoryType::Semantic, 0.85);
        }

        if tasks.len() > 5 {
            let complexity_insight = format!(
                "Insight: Request requiring {} tasks indicates high complexity - consider breaking down similar requests",
                tasks.len()
            );
            self.store_memory_async(complexity_insight, MemoryType::Semantic, 0.7);
        }

        Ok(())
    }

    async fn create_execution_plan(&mut self) -> Result<AgentResponse<ExecutionPlan>, AppError> {
        const MAX_ITERATIONS: usize = 3;
        let mut current_plan: Option<ExecutionPlan> = None;
        let mut iteration = 0;
        let context_search_results: Vec<String> = Vec::new();

        let available_tools: Vec<serde_json::Value> = self
            .agent
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "tool_type": tool.tool_type,
                    "parameters": tool.configuration
                })
            })
            .collect();

        let workflows: Vec<serde_json::Value> = self
            .agent
            .workflows
            .iter()
            .map(|w| {
                json!({
                    "name": w.name,
                    "description": w.description
                })
            })
            .collect();

        let knowledge_bases: Vec<serde_json::Value> = self
            .agent
            .knowledge_bases
            .iter()
            .map(|kb| {
                json!({
                    "name": kb.name,
                    "description": kb.description
                })
            })
            .collect();

        while iteration < MAX_ITERATIONS {
            iteration += 1;

            let planning_context = json!({
                "memories": self.memories.iter().map(|m| m.content.clone()).collect::<Vec<_>>(),
                "context_search_results": context_search_results,
                "available_tools": available_tools,
                "workflows": workflows,
                "knowledge_bases": knowledge_bases,
                "iteration": iteration,
                "max_iterations": MAX_ITERATIONS,
                "is_final_iteration": iteration == MAX_ITERATIONS,
                "current_plan": current_plan.as_ref().map(|p| json!({
                    "message": p.message,
                    "analysis": p.analysis,
                    "tasks": p.tasks,
                    "success_criteria": p.success_criteria
                }))
            });

            let prompt = render_template(AgentTemplates::TASK_PLANNING, &planning_context)
                .map_err(|e| {
                    AppError::Internal(format!("Failed to render planning template: {}", e))
                })?;

            let llm = self.create_strong_llm(Some(&prompt))?;

            tracing::info!(
                "Starting planning iteration {} of {}",
                iteration,
                MAX_ITERATIONS
            );

            let response_text = {
                let mut res = String::new();
                let mut stream = llm.chat_stream(&self.conversations).await?;

                while let Some(Ok(token)) = stream.next().await {
                    res.push_str(&token);
                }

                res
            };

            // Push assistant response to conversations
            self.conversations
                .push(ChatMessage::assistant().content(&response_text).build());

            // Try to parse as planning iteration response first
            if response_text.contains("<planning_iteration_response>") {
                match xml_parser::from_str::<PlanningIterationResponse>(&response_text) {
                    Ok(iter_response) => {
                        current_plan = Some(iter_response.execution_plan);

                        if iter_response.needs_more_iteration && iteration < MAX_ITERATIONS {
                            // Handle context search request if provided
                            if let Some(request) = &iter_response.context_search_request {
                                tracing::info!("Planning requested context search: {}", request);
                                // Could perform actual context search here and add to context_search_results
                            }
                            tracing::info!(
                                "Plan iteration {}: {}",
                                iteration,
                                iter_response.iteration_notes
                            );
                            continue;
                        } else {
                            // Plan is complete
                            tracing::info!("Plan finalized: {}", iter_response.iteration_notes);
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to parse planning iteration response: {}", e);
                        // Fall through to try parsing as direct execution plan
                    }
                }
            }

            // Try to parse as direct execution plan
            match xml_parser::from_str::<ExecutionPlan>(&response_text) {
                Ok(plan) => {
                    current_plan = Some(plan);
                    break;
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to parse execution plan in iteration {}: {}",
                        iteration,
                        e
                    );
                    if iteration == MAX_ITERATIONS {
                        return Err(AppError::Internal(
                            "Failed to create valid execution plan".to_string(),
                        ));
                    }
                }
            }
        }

        let final_plan = current_plan.ok_or_else(|| {
            AppError::Internal("Failed to create execution plan after iterations".to_string())
        })?;

        // Store the tasks
        {
            let mut lock = self.current_tasks.lock().await;
            let mut tasks: Vec<Task> = final_plan.tasks.iter().map(|pt| pt.to_task()).collect();
            lock.append(&mut tasks);
        }

        tracing::info!(
            "Execution plan created with {} tasks after {} iteration(s)",
            final_plan.tasks.len(),
            iteration
        );

        Ok(AgentResponse {
            parsed: final_plan,
            xml_content: String::new(),
        })
    }

    fn spawn_response_learning(&self, xml_response: String, context_id: i64) {
        let app_state = self.app_state.clone();
        let decay_manager = DecayManager::new(app_state.clone());

        tokio::spawn(async move {
            let citations = match CitationExtractor::extract_citations(&xml_response) {
                Ok(cites) => cites,
                Err(e) => {
                    tracing::debug!("No citations found in response: {}", e);
                    Vec::new()
                }
            };

            if let Some(reasoning) = CitationExtractor::extract_reasoning(&xml_response) {
                match GenerateEmbeddingCommand::new(reasoning.clone())
                    .execute(&app_state)
                    .await
                {
                    Ok(reasoning_embedding) => {
                        if !citations.is_empty() {
                            if let Err(e) =
                                decay_manager.update_decay_from_citations(&citations).await
                            {
                                tracing::warn!("Failed to update decay from citations: {}", e);
                            }
                        }

                        // Refine context for future interactions
                        match decay_manager
                            .refine_context_from_reasoning(&reasoning_embedding, context_id, 20)
                            .await
                        {
                            Ok(refined_context) => {
                                tracing::debug!(
                                    "Refined context: {} memories, {} conversations",
                                    refined_context.relevant_memories.len(),
                                    refined_context.relevant_conversations.len()
                                );

                                // TODO: Cache refined context for next interaction
                                // This could be stored in Redis or a similar cache
                            }
                            Err(e) => {
                                tracing::warn!("Failed to refine context: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to generate reasoning embedding: {}", e);
                    }
                }
            }
        });
    }
}
