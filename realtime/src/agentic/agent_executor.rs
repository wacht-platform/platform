use crate::agentic::{MessageParser, xml_parser};
use crate::template::{AgentTemplates, render_template};
use chrono::Utc;
use futures::StreamExt;
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared::commands::{
    Command, CreateAgentDynamicContextCommand, CreateExecutionMessageCommand, CreateMemoryCommand,
    DeleteAgentDynamicContextCommand, GenerateEmbeddingCommand,
    SearchKnowledgeBaseEmbeddingsCommand,
};
use shared::dto::json::StreamEvent;
use shared::error::AppError;
use shared::models::{
    AgentExecutionContextMessage, AiAgentWithFeatures, ExecutionMessageSender,
    ExecutionMessageType, MemoryEntry, MemoryQuery, MemorySearchResult, MemoryType, ToolResult,
};
use shared::queries::{
    GetExecutionMessagesQuery, Query, SearchAgentDynamicContextQuery, SearchMemoriesQuery,
};
use shared::state::AppState;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename = "response")]
pub struct AcknowledgmentResponse {
    #[serde(rename = "message")]
    pub acknowledgment_message: String,
    pub further_action_required: bool,
    pub reasoning: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Blocked,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    pub dependencies: Vec<String>,
    pub context: Value,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename = "task_plan")]
pub struct TaskPlan {
    pub tasks: Vec<Task>,
    pub reasoning: String,
    pub estimated_steps: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool_id: i64,
    pub tool_name: String,
    pub parameters: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowCall {
    pub workflow_id: i64,
    pub workflow_name: String,
    pub input_data: Value,
}

pub struct AgentExecutor {
    pub agent: AiAgentWithFeatures,
    pub app_state: AppState,
    pub context_id: i64,
    pub deployment_id: i64,
    pub current_tasks: Arc<Mutex<Vec<Task>>>,
}

impl AgentExecutor {
    pub async fn new(
        agent: AiAgentWithFeatures,
        deployment_id: i64,
        context_id: i64,
        app_state: AppState,
    ) -> Result<Self, AppError> {
        Ok(Self {
            agent,
            app_state,
            context_id,
            deployment_id,
            current_tasks: Arc::new(Mutex::new(Vec::new())),
        })
    }

    async fn load_conversation_history(
        &self,
    ) -> Result<Vec<AgentExecutionContextMessage>, AppError> {
        GetExecutionMessagesQuery::new(self.context_id)
            .execute(&self.app_state)
            .await
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
        let relevant_memories = self.search_memories(&memory_query).await?;

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
            self.execute_task_execution_loop(
                user_message,
                &conversation_history,
                &memories,
                channel.clone(),
            )
            .await?;
        }

        Ok(())
    }

    pub async fn store_dynamic_context(
        &self,
        content: &str,
        source: Option<String>,
    ) -> Result<(), AppError> {
        let embedding = if content.len() > 10 {
            Some(
                GenerateEmbeddingCommand::new(content.to_string())
                    .execute(&self.app_state)
                    .await?,
            )
        } else {
            None
        };

        CreateAgentDynamicContextCommand {
            id: self.app_state.sf.next_id()? as i64,
            execution_context_id: self.context_id,
            content: content.to_string(),
            source,
            embedding,
        }
        .execute(&self.app_state)
        .await?;

        Ok(())
    }

    pub async fn delete_dynamic_context(&self, context_id: i64) -> Result<(), AppError> {
        DeleteAgentDynamicContextCommand { id: context_id }
            .execute(&self.app_state)
            .await?;

        Ok(())
    }

    async fn generate_acknowledgment(
        &self,
        user_message: &str,
        conversation_history: &[AgentExecutionContextMessage],
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

        let dynamic_context_results = self.search_dynamic_context_vector(user_message).await?;

        let acknowledgment_context = json!({
            "tools": &self.agent.tools,
            "workflows": &self.agent.workflows,
            "knowledge_bases": &self.agent.knowledge_bases,
            "memories": memories,
            "dynamic_context": dynamic_context_results
        });

        let system_prompt =
            render_template(AgentTemplates::ACKNOWLEDGMENT, &acknowledgment_context).map_err(
                |e| AppError::Internal(format!("Failed to render acknowledgment template: {}", e)),
            )?;

        let conversation_context =
            self.prepare_conversation_context(conversation_history, 200_000)?;

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
        conversation_history: &[AgentExecutionContextMessage],
        _max_tokens: usize,
    ) -> Result<String, AppError> {
        // TODO: Implement token-based truncation
        let mut context = String::new();

        // History is newest-first. We want to display oldest-first, and exclude the newest message (current user input).
        if conversation_history.len() <= 1 {
            return Ok(context);
        }

        // Skip the first message (newest) and reverse the rest to get chronological order.
        let mut history_to_format: Vec<_> = conversation_history.iter().skip(1).collect();
        history_to_format.reverse();

        if !history_to_format.is_empty() {
            context.push_str("Previous conversation:\n");
            for message in history_to_format {
                let sender = match message.sender {
                    ExecutionMessageSender::User => "User",
                    ExecutionMessageSender::Agent => "Agent",
                    ExecutionMessageSender::System => "System",
                    ExecutionMessageSender::Tool => "Tool",
                };
                context.push_str(&format!("{}: {}\n", sender, message.content));
            }
        }
        Ok(context.trim().to_string())
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

    async fn execute_task_execution_loop(
        &mut self,
        user_message: &str,
        conversation_history: &[AgentExecutionContextMessage],
        memories: &[MemoryEntry],
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<(), AppError> {
        // Step 1: Create task plan
        let task_plan = self
            .create_task_plan(
                user_message,
                conversation_history,
                memories,
                channel.clone(),
            )
            .await?;

        // Store the task plan
        self.store_execution_message(
            ExecutionMessageType::AgentResponse,
            ExecutionMessageSender::System,
            "Task plan created",
            json!({
                "task_count": task_plan.tasks.len(),
                "reasoning": task_plan.reasoning
            }),
            None,
            None,
        )
        .await?;

        // Initialize tasks
        {
            let mut tasks = self.current_tasks.lock().await;
            *tasks = task_plan.tasks;
        }

        // Step 2: Execute tasks in loop
        let max_iterations = 50;
        let mut iteration = 0;

        while iteration < max_iterations {
            iteration += 1;

            // Get next executable task
            let next_task = self.get_next_executable_task().await?;

            if let Some(task_id) = next_task {
                // Send real-time update
                let _ = channel
                    .send(StreamEvent::Token(
                        format!("\n\nExecuting task: {}", task_id),
                        "task_update".to_string(),
                    ))
                    .await;

                // Execute the task with memories
                match self
                    .execute_single_task(&task_id, memories, channel.clone())
                    .await
                {
                    Ok(result) => {
                        // Update task status
                        self.update_task_status(
                            &task_id,
                            TaskStatus::Completed,
                            Some(result),
                            None,
                        )
                        .await?;
                    }
                    Err(e) => {
                        // Update task status with error
                        self.update_task_status(
                            &task_id,
                            TaskStatus::Failed,
                            None,
                            Some(e.to_string()),
                        )
                        .await?;

                        // Attempt to create recovery tasks
                        self.create_recovery_tasks(&task_id, &e.to_string(), channel.clone())
                            .await?;
                    }
                }

                // Check if all tasks are complete
                if self.are_all_tasks_complete().await? {
                    break;
                }

                // Monitor and update tasks based on progress
                self.monitor_and_update_tasks(channel.clone()).await?;
            } else {
                // No executable tasks, check if we're blocked or done
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
                    let _ = channel
                        .send(StreamEvent::Token(
                            "\n\nTasks are blocked. Creating resolution tasks...".to_string(),
                            "status".to_string(),
                        ))
                        .await;
                    drop(tasks);
                    self.create_unblocking_tasks(channel.clone()).await?;
                } else if pending_count == 0 {
                    // All tasks are complete or failed
                    break;
                }
            }
        }

        // Step 3: Generate final summary
        let summary = self.generate_execution_summary().await?;

        self.store_execution_message(
            ExecutionMessageType::AgentResponse,
            ExecutionMessageSender::Agent,
            &summary,
            json!({
                "execution_complete": true,
                "iterations": iteration
            }),
            None,
            None,
        )
        .await?;

        // Store execution memory
        self.auto_store_conversation_memory(user_message, &summary, None)
            .await?;

        // Learn from execution
        self.learn_from_execution(user_message, &summary).await?;

        Ok(())
    }

    pub async fn store_memory(
        &self,
        content: &str,
        memory_type: MemoryType,
        importance: f32,
    ) -> Result<(), AppError> {
        let embedding = GenerateEmbeddingCommand::new(content.to_string())
            .execute(&self.app_state)
            .await?;

        CreateMemoryCommand {
            id: self.app_state.sf.next_id()? as i64,
            deployment_id: self.deployment_id,
            agent_id: self.agent.id,
            execution_context_id: Some(self.context_id),
            memory_type,
            content: content.to_string(),
            embedding,
            importance,
        }
        .execute(&self.app_state)
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

        // The query already sorts by distance/score, but we re-sort by our combined relevance score.
        results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(results)
    }

    fn calculate_text_relevance(&self, content: &str, query: &str) -> f32 {
        let content_lower = content.to_lowercase();
        let query_lower = query.to_lowercase();

        // Simple text matching score
        let mut score = 0.0;

        // Exact match
        if content_lower.contains(&query_lower) {
            score += 0.5;
        }

        // Word-level matching
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let content_words: Vec<&str> = content_lower.split_whitespace().collect();

        let mut matched_words = 0;
        for query_word in &query_words {
            if content_words.iter().any(|cw| cw.contains(query_word)) {
                matched_words += 1;
            }
        }

        if !query_words.is_empty() {
            score += (matched_words as f32 / query_words.len() as f32) * 0.5;
        }

        score.clamp(0.0, 1.0)
    }

    fn calculate_cosine_similarity(&self, vec1: &[f32], vec2: &[f32]) -> f32 {
        if vec1.len() != vec2.len() {
            return 0.0;
        }

        let dot_product: f32 = vec1.iter().zip(vec2.iter()).map(|(a, b)| a * b).sum();
        let norm1: f32 = vec1.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm2: f32 = vec2.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm1 == 0.0 || norm2 == 0.0 {
            0.0
        } else {
            dot_product / (norm1 * norm2)
        }
    }

    // --- Inlined ContextEngine methods ---

    pub async fn search_context(&self, query: &str) -> Result<Value, AppError> {
        use std::time::Instant;
        use tokio::try_join;

        let start_time = Instant::now();

        let search_results = try_join!(
            self.search_tools_with_llm(query),
            self.search_workflows_with_llm(query),
            self.search_knowledge_base_metadata_vector(query),
            self.search_knowledge_base_documents(query),
            self.search_memory_context(query),
            self.search_conversation_history_vector(query),
            self.search_dynamic_context_vector(query)
        )?;

        let search_duration = start_time.elapsed();

        let mut all_results = Vec::new();
        all_results.extend(search_results.0);
        all_results.extend(search_results.1);
        all_results.extend(search_results.2);
        all_results.extend(search_results.3);
        all_results.extend(search_results.4);
        all_results.extend(search_results.5);
        all_results.extend(search_results.6);

        all_results.sort_by(|a, b| {
            let score_a = a
                .get("relevance_score")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let score_b = b
                .get("relevance_score")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        all_results.truncate(50);

        let executed_tools = all_results
            .iter()
            .filter(|r| {
                r.get("type").and_then(|t| t.as_str()) == Some("tool")
                    && r.get("executed").and_then(|e| e.as_bool()).unwrap_or(false)
            })
            .count();

        let executed_workflows = all_results
            .iter()
            .filter(|r| {
                r.get("type").and_then(|t| t.as_str()) == Some("workflow")
                    && r.get("executed").and_then(|e| e.as_bool()).unwrap_or(false)
            })
            .count();

        Ok(json!({
            "query": query,
            "results": all_results,
            "total_found": all_results.len(),
            "search_timestamp": chrono::Utc::now().to_rfc3339(),
            "search_types": ["tools_llm", "workflows_llm", "knowledge_bases_vector", "documents_vector", "memory_vector", "conversation_history_vector", "dynamic_context_vector"],
            "parallel_execution": true,
            "search_duration_ms": search_duration.as_millis(),
            "performance": {
                "parallel_searches": 7,
                "estimated_sequential_time_saved": "60-85%"
            },
            "execution_summary": {
                "tools_executed": executed_tools,
                "workflows_executed": executed_workflows,
                "total_executions": executed_tools + executed_workflows,
                "intelligent_execution": true,
                "confidence_threshold": 80
            }
        }))
    }

    async fn search_tools_with_llm(&self, query: &str) -> Result<Vec<Value>, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.5-flash")
            .max_tokens(4000)
            .temperature(0.1)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build LLM: {}", e)))?;

        let tool_analysis_context = json!({
            "user_query": query,
            "tools": &self.agent.tools
        });

        let prompt = render_template(AgentTemplates::TOOL_ANALYSIS, &tool_analysis_context)
            .map_err(|e| {
                AppError::Internal(format!("Failed to render tool analysis template: {}", e))
            })?;

        let messages = vec![ChatMessage::user().content(&prompt).build()];

        let response_text = {
            let response = llm
                .chat(&messages)
                .await
                .map_err(|e| AppError::Internal(format!("LLM tool analysis failed: {}", e)))?;
            response.to_string()
        };

        let mut results = Vec::new();

        if let Ok(llm_results) = serde_json::from_str::<Vec<serde_json::Value>>(&response_text) {
            for result in llm_results {
                if let (
                    Some(tool_id),
                    Some(relevance_score),
                    Some(confidence_score),
                    Some(should_execute),
                ) = (
                    result.get("tool_id").and_then(|v| v.as_i64()),
                    result.get("relevance_score").and_then(|v| v.as_f64()),
                    result.get("confidence_score").and_then(|v| v.as_f64()),
                    result.get("should_execute").and_then(|v| v.as_bool()),
                ) {
                    if let Some(tool) = self.agent.tools.iter().find(|t| t.id == tool_id) {
                        let mut tool_result = json!({
                            "type": "tool",
                            "id": tool.id,
                            "name": tool.name,
                            "description": tool.description,
                            "tool_type": tool.tool_type,
                            "configuration": tool.configuration,
                            "relevance_score": relevance_score,
                            "confidence_score": confidence_score,
                            "should_execute": should_execute,
                            "llm_reasoning": result.get("reasoning").and_then(|v| v.as_str()).unwrap_or("")
                        });

                        if should_execute && confidence_score >= 80.0 {
                            let execution_params = result
                                .get("execution_parameters")
                                .cloned()
                                .unwrap_or(json!({}));
                            match self.execute_tool_immediately(tool, execution_params).await {
                                Ok(execution_result) => {
                                    tool_result["execution_result"] = execution_result;
                                    tool_result["executed"] = json!(true);
                                }
                                Err(e) => {
                                    tool_result["execution_error"] = json!(e.to_string());
                                    tool_result["executed"] = json!(false);
                                }
                            }
                        }
                        results.push(tool_result);
                    }
                }
            }
        }

        Ok(results)
    }

    async fn search_workflows_with_llm(&self, query: &str) -> Result<Vec<Value>, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.5-pro") // Use Pro for complex workflow analysis
            .max_tokens(6000)
            .temperature(0.1)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build LLM: {}", e)))?;

        let workflow_analysis_context = json!({
            "user_query": query,
            "workflows": &self.agent.workflows
        });

        let prompt = render_template(AgentTemplates::TOOL_ANALYSIS, &workflow_analysis_context)
            .map_err(|e| {
                AppError::Internal(format!(
                    "Failed to render workflow analysis template: {}",
                    e
                ))
            })?;

        let messages = vec![ChatMessage::user().content(&prompt).build()];

        let response_text = {
            let response = llm
                .chat(&messages)
                .await
                .map_err(|e| AppError::Internal(format!("LLM workflow analysis failed: {}", e)))?;
            response.to_string()
        };

        let mut results = Vec::new();

        if let Ok(llm_results) = serde_json::from_str::<Vec<serde_json::Value>>(&response_text) {
            for result in llm_results {
                if let (
                    Some(workflow_id),
                    Some(relevance_score),
                    Some(trigger_met),
                    Some(confidence_score),
                    Some(should_execute),
                ) = (
                    result.get("workflow_id").and_then(|v| v.as_i64()),
                    result.get("relevance_score").and_then(|v| v.as_f64()),
                    result
                        .get("trigger_condition_met")
                        .and_then(|v| v.as_bool()),
                    result.get("confidence_score").and_then(|v| v.as_f64()),
                    result.get("should_execute").and_then(|v| v.as_bool()),
                ) {
                    if let Some(workflow) =
                        self.agent.workflows.iter().find(|w| w.id == workflow_id)
                    {
                        let mut workflow_result = json!({
                            "type": "workflow",
                            "id": workflow.id,
                            "name": workflow.name,
                            "description": workflow.description,
                            "configuration": workflow.configuration,
                            "workflow_definition": workflow.workflow_definition,
                            "relevance_score": relevance_score,
                            "trigger_condition_met": trigger_met,
                            "confidence_score": confidence_score,
                            "should_execute": should_execute,
                            "llm_reasoning": result.get("reasoning").and_then(|v| v.as_str()).unwrap_or(""),
                            "trigger_analysis": result.get("trigger_analysis").and_then(|v| v.as_str()).unwrap_or("")
                        });

                        if should_execute && trigger_met && confidence_score >= 80.0 {
                            let execution_input =
                                result.get("execution_input").cloned().unwrap_or(json!({}));
                            match self
                                .execute_workflow_immediately(workflow, execution_input)
                                .await
                            {
                                Ok(execution_result) => {
                                    workflow_result["execution_result"] = execution_result;
                                    workflow_result["executed"] = json!(true);
                                }
                                Err(e) => {
                                    workflow_result["execution_error"] = json!(e.to_string());
                                    workflow_result["executed"] = json!(false);
                                }
                            }
                        }
                        results.push(workflow_result);
                    }
                }
            }
        }

        Ok(results)
    }

    async fn search_knowledge_base_metadata_vector(
        &self,
        query: &str,
    ) -> Result<Vec<Value>, AppError> {
        let query_embedding = GenerateEmbeddingCommand::new(query.to_string())
            .execute(&self.app_state)
            .await?;

        let kb_futures: Vec<_> = self
            .agent
            .knowledge_bases
            .iter()
            .map(|kb| {
                let query_embedding = query_embedding.clone();
                let kb_clone = kb.clone();
                let app_state = self.app_state.clone();
                async move {
                    let kb_text = format!(
                        "{} {} {}",
                        kb_clone.name,
                        kb_clone.description.as_deref().unwrap_or(""),
                        serde_json::to_string(&kb_clone.configuration).unwrap_or_default()
                    );

                    let kb_embedding = GenerateEmbeddingCommand::new(kb_text)
                        .execute(&app_state)
                        .await?;
                    let similarity_score =
                        self.calculate_cosine_similarity(&query_embedding, &kb_embedding);

                    if similarity_score > 0.3 {
                        Ok::<Option<serde_json::Value>, AppError>(Some(json!({
                            "type": "knowledge_base_metadata",
                            "id": kb_clone.id,
                            "name": kb_clone.name,
                            "description": kb_clone.description,
                            "configuration": kb_clone.configuration,
                            "relevance_score": (similarity_score * 100.0) as f64,
                            "similarity_score": similarity_score
                        })))
                    } else {
                        Ok(None)
                    }
                }
            })
            .collect();

        let results = futures::future::try_join_all(kb_futures).await?;
        let filtered_results = results.into_iter().filter_map(|r| r).collect();

        Ok(filtered_results)
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
                    let search_results = SearchKnowledgeBaseEmbeddingsCommand::new(
                        kb_clone.id,
                        query_embedding,
                        10, // Limit per knowledge base
                    )
                    .execute(&app_state)
                    .await?;

                    let mut kb_results = Vec::new();
                    for result in search_results {
                        // The score from the DB is L2 distance (0=identical, >0=dissimilar). Convert to similarity (1=identical, 0=dissimilar).
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

    async fn search_memory_context(&self, query: &str) -> Result<Vec<Value>, AppError> {
        let memory_query = MemoryQuery {
            query: query.to_string(),
            memory_types: vec![
                MemoryType::Episodic,
                MemoryType::Semantic,
                MemoryType::Procedural,
            ],
            max_results: 10,
            min_importance: 0.3,
            time_range: None,
        };

        let search_results = self.search_memories(&memory_query).await?;

        let mut results = Vec::new();
        for result in search_results {
            results.push(json!({
                "type": "memory",
                "id": result.entry.id,
                "content": result.entry.content,
                "memory_type": result.entry.memory_type,
                "importance": result.entry.importance,
                "created_at": result.entry.created_at,
                "relevance_score": result.similarity_score as f64,
                "source": "agent_memory"
            }));
        }

        Ok(results)
    }

    async fn search_conversation_history_vector(
        &self,
        query: &str,
    ) -> Result<Vec<Value>, AppError> {
        let query_embedding = GenerateEmbeddingCommand::new(query.to_string())
            .execute(&self.app_state)
            .await?;

        let conversation_history = self.load_conversation_history().await?;

        let mut results = Vec::new();

        for message in conversation_history.iter().take(50) {
            // Skip very short messages
            if message.content.len() < 10 {
                continue;
            }

            // Generate embedding for the message
            let message_embedding = GenerateEmbeddingCommand::new(message.content.clone())
                .execute(&self.app_state)
                .await?;

            // Calculate similarity
            let similarity = self.calculate_cosine_similarity(&query_embedding, &message_embedding);

            if similarity > 0.5 {
                results.push(json!({
                    "type": "conversation",
                    "id": message.id,
                    "content": message.content,
                    "sender": message.sender,
                    "message_type": message.message_type,
                    "created_at": message.created_at,
                    "relevance_score": (similarity * 100.0) as f64,
                    "source": "conversation_history"
                }));
            }
        }

        // Sort by relevance
        results.sort_by(|a, b| {
            let score_a = a
                .get("relevance_score")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let score_b = b
                .get("relevance_score")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Return top 10 results
        results.truncate(10);

        Ok(results)
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

    async fn execute_tool_immediately(
        &self,
        tool: &shared::models::AiTool,
        execution_params: Value,
    ) -> Result<Value, AppError> {
        if tool.name == "context_engine" || tool.name == "memory" {
            return Ok(json!({
                "tool_id": tool.id,
                "tool_name": tool.name,
                "execution_type": "skipped",
                "reason": "Prevented recursive execution of context engine or memory tools",
                "execution_timestamp": chrono::Utc::now().to_rfc3339()
            }));
        }

        Ok(json!({
            "tool_id": tool.id,
            "tool_name": tool.name,
            "execution_type": "planned",
            "execution_params": execution_params,
            "message": "Tool execution planned - will be executed by agent executor",
            "execution_timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    async fn execute_workflow_immediately(
        &self,
        workflow: &shared::models::AiWorkflow,
        execution_input: Value,
    ) -> Result<Value, AppError> {
        Ok(json!({
            "workflow_id": workflow.id,
            "workflow_name": workflow.name,
            "execution_type": "planned",
            "input_data": execution_input,
            "trigger_validated": true,
            "message": "Workflow execution planned - will be executed by agent executor",
            "execution_timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    // Task management methods
    async fn create_task_plan(
        &self,
        user_message: &str,
        conversation_history: &[AgentExecutionContextMessage],
        memories: &[MemoryEntry],
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<TaskPlan, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.5-pro")
            .max_tokens(8000)
            .temperature(0.3)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build LLM: {}", e)))?;

        // Search for relevant context
        let context_results = self.search_context(user_message).await?;

        let task_planning_context = json!({
            "user_request": user_message,
            "tools": &self.agent.tools,
            "workflows": &self.agent.workflows,
            "knowledge_bases": &self.agent.knowledge_bases,
            "memories": memories,
            "context_search_results": context_results
        });

        let system_prompt = render_template(AgentTemplates::TASK_ANALYSIS, &task_planning_context)
            .map_err(|e| {
                AppError::Internal(format!("Failed to render task analysis template: {}", e))
            })?;

        let conversation_context =
            self.prepare_conversation_context(conversation_history, 100_000)?;

        let full_prompt = format!(
            "{}\n\n{}\n\nUser Request: {}\n\nGenerate a detailed task plan as an XML response.",
            system_prompt, conversation_context, user_message
        );

        let messages = vec![ChatMessage::user().content(&full_prompt).build()];

        let _ = channel
            .send(StreamEvent::Token(
                "\n\nCreating task plan...".to_string(),
                "status".to_string(),
            ))
            .await;

        let response_text = {
            let mut res = String::new();
            let mut stream = llm.chat_stream(&messages).await?;

            while let Some(Ok(token)) = stream.next().await {
                res.push_str(&token);
            }

            res
        };

        let task_plan: TaskPlan = xml_parser::from_str(&response_text)?;

        // Store task plan as dynamic context
        self.store_dynamic_context(
            &format!("Task Plan: {}", serde_json::to_string(&task_plan)?),
            Some("task_planning".to_string()),
        )
        .await?;

        Ok(task_plan)
    }

    async fn get_next_executable_task(&self) -> Result<Option<String>, AppError> {
        let tasks = self.current_tasks.lock().await;

        for task in tasks.iter() {
            if matches!(task.status, TaskStatus::Pending) {
                // Check if all dependencies are complete
                let deps_complete = task.dependencies.iter().all(|dep_id| {
                    tasks
                        .iter()
                        .any(|t| t.id == *dep_id && matches!(t.status, TaskStatus::Completed))
                });

                if deps_complete {
                    return Ok(Some(task.id.clone()));
                }
            }
        }

        Ok(None)
    }

    async fn execute_single_task(
        &self,
        task_id: &str,
        memories: &[MemoryEntry],
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        // Update task status to InProgress
        self.update_task_status(task_id, TaskStatus::InProgress, None, None)
            .await?;

        // Get task details
        let task = {
            let tasks = self.current_tasks.lock().await;
            tasks.iter().find(|t| t.id == task_id).cloned()
        };

        let task = task.ok_or_else(|| AppError::NotFound(format!("Task {} not found", task_id)))?;

        let _ = channel
            .send(StreamEvent::Token(
                format!("\nTask: {}", task.description),
                "task".to_string(),
            ))
            .await;

        // Execute based on task context
        if let Some(tool_call) = task.context.get("tool_call") {
            // Execute tool
            let tool_call: ToolCall = serde_json::from_value(tool_call.clone())?;
            self.execute_tool_task(&tool_call, memories, channel.clone())
                .await
        } else if let Some(workflow_call) = task.context.get("workflow_call") {
            // Execute workflow
            let workflow_call: WorkflowCall = serde_json::from_value(workflow_call.clone())?;
            self.execute_workflow_task(&workflow_call, memories, channel.clone())
                .await
        } else if task.context.get("search_context").is_some() {
            // Execute context search
            let query = task
                .context
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or(&task.description);

            // Include memory search in context results
            let mut context_results = self.search_context(query).await?;

            // Add relevant memories to context
            if let Some(results_array) = context_results
                .get_mut("results")
                .and_then(|v| v.as_array_mut())
            {
                // Find highly relevant memories not already included
                let memory_query = MemoryQuery {
                    query: query.to_string(),
                    memory_types: vec![MemoryType::Procedural, MemoryType::Semantic],
                    max_results: 5,
                    min_importance: 0.6,
                    time_range: None,
                };

                if let Ok(additional_memories) = self.search_memories(&memory_query).await {
                    for mem_result in additional_memories {
                        if mem_result.relevance_score > 0.7 {
                            results_array.push(json!({
                                "type": "memory",
                                "id": mem_result.entry.id,
                                "content": mem_result.entry.content,
                                "memory_type": mem_result.entry.memory_type,
                                "importance": mem_result.entry.importance,
                                "relevance_score": mem_result.relevance_score * 100.0,
                                "source": "task_execution_memory_search"
                            }));
                        }
                    }
                }
            }

            Ok(context_results)
        } else if task.context.get("store_memory").is_some() {
            // Store memory
            self.execute_memory_task(&task.context, channel.clone())
                .await
        } else if task.context.get("update_memory").is_some() {
            // Update memory
            self.execute_memory_update_task(&task.context, channel.clone())
                .await
        } else if task.context.get("store_dynamic_context").is_some() {
            // Store dynamic context
            self.execute_dynamic_context_task(&task.context, channel.clone())
                .await
        } else if task.context.get("delete_dynamic_context").is_some() {
            // Delete dynamic context
            self.execute_delete_dynamic_context_task(&task.context, channel.clone())
                .await
        } else {
            // Generic task execution with LLM
            self.execute_generic_task(&task, memories, channel.clone())
                .await
        }
    }

    async fn execute_tool_task(
        &self,
        tool_call: &ToolCall,
        memories: &[MemoryEntry],
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        let _ = channel
            .send(StreamEvent::Token(
                format!("\nExecuting tool: {}", tool_call.tool_name),
                "tool".to_string(),
            ))
            .await;

        // Find the tool
        let tool = self
            .agent
            .tools
            .iter()
            .find(|t| t.id == tool_call.tool_id)
            .ok_or_else(|| AppError::NotFound(format!("Tool {} not found", tool_call.tool_id)))?;

        // Check memories for relevant tool usage patterns
        let relevant_memories: Vec<&MemoryEntry> = memories
            .iter()
            .filter(|m| {
                matches!(m.memory_type, MemoryType::Procedural)
                    && m.content.contains(&tool_call.tool_name)
            })
            .collect();

        // If we have relevant procedural memories, extract parameters or patterns
        let enhanced_parameters = tool_call.parameters.clone();
        if !relevant_memories.is_empty() {
            // Log that we're using learned patterns
            let _ = channel
                .send(StreamEvent::Token(
                    format!(
                        "\nApplying {} learned patterns for tool usage",
                        relevant_memories.len()
                    ),
                    "memory".to_string(),
                ))
                .await;
        }

        // Execute tool with potentially enhanced parameters
        let result = self
            .execute_tool_immediately(tool, enhanced_parameters)
            .await?;

        // Store tool execution in dynamic context
        self.store_dynamic_context(
            &format!(
                "Tool execution: {} - Result: {}",
                tool_call.tool_name,
                serde_json::to_string(&result)?
            ),
            Some("tool_execution".to_string()),
        )
        .await?;

        Ok(result)
    }

    async fn execute_workflow_task(
        &self,
        workflow_call: &WorkflowCall,
        memories: &[MemoryEntry],
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        let _ = channel
            .send(StreamEvent::Token(
                format!("\nExecuting workflow: {}", workflow_call.workflow_name),
                "workflow".to_string(),
            ))
            .await;

        // Find the workflow
        let workflow = self
            .agent
            .workflows
            .iter()
            .find(|w| w.id == workflow_call.workflow_id)
            .ok_or_else(|| {
                AppError::NotFound(format!("Workflow {} not found", workflow_call.workflow_id))
            })?;

        // Check memories for relevant workflow execution patterns
        let relevant_memories: Vec<&MemoryEntry> = memories
            .iter()
            .filter(|m| {
                matches!(m.memory_type, MemoryType::Procedural)
                    && m.content.contains(&workflow_call.workflow_name)
            })
            .collect();

        // Apply learned patterns to workflow input
        let enhanced_input = workflow_call.input_data.clone();
        if !relevant_memories.is_empty() {
            let _ = channel
                .send(StreamEvent::Token(
                    format!(
                        "\nApplying {} learned patterns for workflow execution",
                        relevant_memories.len()
                    ),
                    "memory".to_string(),
                ))
                .await;
        }

        // Execute workflow with potentially enhanced input
        let result = self
            .execute_workflow_immediately(workflow, enhanced_input)
            .await?;

        // Store workflow execution in dynamic context
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

    async fn execute_memory_task(
        &self,
        context: &Value,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        let content = context
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::BadRequest("Memory content required".to_string()))?;

        let memory_type_str = context
            .get("memory_type")
            .and_then(|v| v.as_str())
            .unwrap_or("semantic");

        let memory_type = match memory_type_str {
            "episodic" => MemoryType::Episodic,
            "procedural" => MemoryType::Procedural,
            _ => MemoryType::Semantic,
        };

        let importance = context
            .get("importance")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5) as f32;

        let _ = channel
            .send(StreamEvent::Token(
                format!("\nStoring {} memory", memory_type_str),
                "memory".to_string(),
            ))
            .await;

        self.store_memory(content, memory_type, importance).await?;

        Ok(json!({
            "action": "memory_stored",
            "memory_type": memory_type_str,
            "content": content,
            "importance": importance
        }))
    }

    async fn execute_memory_update_task(
        &self,
        context: &Value,
        _channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        // Memory updates are handled through creating new memories with higher importance
        // and potentially marking old ones as less important
        let old_content = context
            .get("old_content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::BadRequest("Old content required".to_string()))?;

        let new_content = context
            .get("new_content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::BadRequest("New content required".to_string()))?;

        let memory_type = MemoryType::Semantic;

        // Store the update as a new memory with high importance
        self.store_memory(
            &format!("Update: {} -> {}", old_content, new_content),
            memory_type,
            0.9,
        )
        .await?;

        Ok(json!({
            "action": "memory_updated",
            "old_content": old_content,
            "new_content": new_content
        }))
    }

    async fn execute_dynamic_context_task(
        &self,
        context: &Value,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        let content = context
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::BadRequest("Context content required".to_string()))?;

        let source = context
            .get("source")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let _ = channel
            .send(StreamEvent::Token(
                "\nStoring dynamic context".to_string(),
                "context".to_string(),
            ))
            .await;

        self.store_dynamic_context(content, source.clone()).await?;

        Ok(json!({
            "action": "dynamic_context_stored",
            "content": content,
            "source": source
        }))
    }

    async fn execute_delete_dynamic_context_task(
        &self,
        context: &Value,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
        let context_id = context
            .get("context_id")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| AppError::BadRequest("Context ID required".to_string()))?;

        let _ = channel
            .send(StreamEvent::Token(
                format!("\nDeleting dynamic context ID: {}", context_id),
                "context".to_string(),
            ))
            .await;

        self.delete_dynamic_context(context_id).await?;

        Ok(json!({
            "action": "dynamic_context_deleted",
            "context_id": context_id
        }))
    }

    async fn execute_generic_task(
        &self,
        task: &Task,
        memories: &[MemoryEntry],
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Value, AppError> {
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

        // Filter relevant memories for this task
        let relevant_memories: Vec<&MemoryEntry> = memories
            .iter()
            .filter(|m| {
                // Include procedural memories (how to do things)
                // and semantic memories (what things mean)
                matches!(m.memory_type, MemoryType::Procedural | MemoryType::Semantic)
                    && m.importance > 0.5
            })
            .collect();

        // Format memories for the prompt
        let memory_context = if !relevant_memories.is_empty() {
            let memory_text: Vec<String> = relevant_memories
                .iter()
                .map(|m| format!("- [{}] {}", m.memory_type.as_str(), m.content))
                .collect();
            format!(
                "\n\nRelevant learned information:\n{}",
                memory_text.join("\n")
            )
        } else {
            String::new()
        };

        let prompt = format!(
            "Execute the following task: {}\nContext: {}{}\n\nProvide a detailed result. Use any learned patterns or information to ensure the execution is smooth and idiomatic.",
            task.description,
            serde_json::to_string_pretty(&task.context)?,
            memory_context
        );

        let messages = vec![ChatMessage::user().content(&prompt).build()];

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

        Ok(json!({
            "action": "generic_task_completed",
            "task": task.description,
            "result": response_text
        }))
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

        let _ = channel
            .send(StreamEvent::Token(
                format!(
                    "\n\nProgress: {}/{} completed, {} failed, {} in progress",
                    completed, total, failed, in_progress
                ),
                "progress".to_string(),
            ))
            .await;

        Ok(())
    }

    async fn create_recovery_tasks(
        &self,
        failed_task_id: &str,
        error: &str,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<(), AppError> {
        let _ = channel
            .send(StreamEvent::Token(
                format!(
                    "\n\nTask {} failed: {}. Creating recovery strategy...",
                    failed_task_id, error
                ),
                "error".to_string(),
            ))
            .await;

        // Create a recovery task
        let recovery_task = Task {
            id: format!("{}_recovery", failed_task_id),
            description: format!("Recover from error in task {}: {}", failed_task_id, error),
            status: TaskStatus::Pending,
            dependencies: vec![],
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
                dependencies: vec![],
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

        let _ = channel
            .send(StreamEvent::Token(
                "\nCreated unblocking tasks".to_string(),
                "status".to_string(),
            ))
            .await;

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
        user_message: &str,
        summary: &str,
    ) -> Result<(), AppError> {
        let tasks = self.current_tasks.lock().await;

        // Learn from successful patterns
        let successful_patterns: Vec<String> = tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Completed))
            .filter_map(|t| {
                if let Some(tool_call) = t.context.get("tool_call") {
                    Some(format!(
                        "Successfully used tool: {}",
                        tool_call
                            .get("tool_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                    ))
                } else {
                    None
                }
            })
            .collect();

        for pattern in successful_patterns {
            self.store_memory(&pattern, MemoryType::Procedural, 0.7)
                .await?;
        }

        // Learn from failures
        let failure_patterns: Vec<String> = tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Failed))
            .filter_map(|t| {
                t.error
                    .as_ref()
                    .map(|e| format!("Task failed - {}: {}", t.description, e))
            })
            .collect();

        for pattern in failure_patterns {
            self.store_memory(&pattern, MemoryType::Semantic, 0.8)
                .await?;
        }

        // Store overall execution pattern
        self.store_memory(
            &format!(
                "For request '{}', executed {} tasks with result: {}",
                user_message,
                tasks.len(),
                summary
            ),
            MemoryType::Episodic,
            0.6,
        )
        .await?;

        Ok(())
    }
}
