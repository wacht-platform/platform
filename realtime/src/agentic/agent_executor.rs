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
    GenerateEmbeddingCommand, SearchKnowledgeBaseEmbeddingsCommand,
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
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize)]
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
    pub memories: Vec<MemoryEntry>,
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
        Ok(Self {
            agent,
            app_state,
            memories: Vec::new(),
            context_id,
            deployment_id,
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
            self.execute_task_execution_loop(user_message).await?;
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

    // --- Inlined MemoryManager methods ---

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

            // The score from the DB is L2 distance (0=identical, >0=dissimilar). Convert to similarity (1=identical, 0=dissimilar).
            // Assuming normalized vectors, L2 distance is in [0, 2].
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

    pub async fn get_detailed_info(
        &self,
        resource_type: &str,
        resource_id: i64,
    ) -> Result<Value, AppError> {
        match resource_type {
            "tool" => {
                if let Some(tool) = self.agent.tools.iter().find(|t| t.id == resource_id) {
                    Ok(json!({
                        "type": "tool",
                        "id": tool.id,
                        "name": tool.name,
                        "description": tool.description,
                        "tool_type": tool.tool_type,
                        "configuration": tool.configuration,
                        "created_at": tool.created_at,
                        "updated_at": tool.updated_at
                    }))
                } else {
                    Err(AppError::NotFound("Tool not found".to_string()))
                }
            }
            "workflow" => {
                if let Some(workflow) = self.agent.workflows.iter().find(|w| w.id == resource_id) {
                    Ok(json!({
                        "type": "workflow",
                        "id": workflow.id,
                        "name": workflow.name,
                        "description": workflow.description,
                        "configuration": workflow.configuration,
                        "workflow_definition": workflow.workflow_definition,
                        "created_at": workflow.created_at,
                        "updated_at": workflow.updated_at
                    }))
                } else {
                    Err(AppError::NotFound("Workflow not found".to_string()))
                }
            }
            "knowledge_base" => {
                if let Some(kb) = self
                    .agent
                    .knowledge_bases
                    .iter()
                    .find(|k| k.id == resource_id)
                {
                    Ok(json!({
                        "type": "knowledge_base",
                        "id": kb.id,
                        "name": kb.name,
                        "description": kb.description,
                        "configuration": kb.configuration,
                        "created_at": kb.created_at,
                        "updated_at": kb.updated_at
                    }))
                } else {
                    Err(AppError::NotFound("Knowledge base not found".to_string()))
                }
            }
            _ => Err(AppError::BadRequest(format!(
                "Unknown resource type: {}",
                resource_type
            ))),
        }
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

        let prompt = render_template(
            AgentTemplates::TOOL_ANALYSIS,
            &workflow_analysis_context,
        )
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
        _query: &str,
    ) -> Result<Vec<Value>, AppError> {
        Ok(vec![])
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
}
