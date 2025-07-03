use std::convert;

use super::{MemoryEntry, MemoryType, ToolCall, ToolResult};
use crate::agentic::{xml_parser, MessageParser};
use crate::template::{render_template, AgentTemplates};
use chrono::Utc;
use futures::StreamExt;
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use shared::commands::{
    Command, CreateExecutionMessageCommand, GenerateEmbeddingCommand, StoreMemoryEmbeddingCommand,
};
use shared::dto::json::StreamEvent;
use shared::error::AppError;
use shared::models::{
    AgentExecutionContextMessage, AiAgentWithFeatures, ExecutionMessageSender, ExecutionMessageType,
    MemoryQuery, MemorySearchResult,
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
    pub memories: Vec<MemoryEntry>,
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
        Ok(Self {
            agent,
            app_state,
            memories: Vec::new(),
            message_history: Vec::new(),
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
        let mut metadata = HashMap::new();

        metadata.insert(
            "deployment_id".to_string(),
            serde_json::Value::Number(self.deployment_id.into()),
        );

        metadata.insert(
            "context_id".to_string(),
            serde_json::Value::Number(self.context_id.into()),
        );

        let embedding = GenerateEmbeddingCommand::new(content.to_string())
            .execute(&self.app_state)
            .await?;

        let memory_entry = MemoryEntry {
            id: self.app_state.sf.next_id()? as i64,
            memory_type,
            content: content.to_string(),
            metadata,
            importance,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
            embedding,
        };

        dbg!(&memory_entry);

        self.store_memory_entry(&memory_entry).await?;

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
        let stored_memories = self.get_stored_memories().await?;

        let mut results = Vec::new();

        for memory in stored_memories {
            if !query.memory_types.is_empty()
                && !self.memory_type_matches(&memory.memory_type, &query.memory_types)
            {
                continue;
            }

            if memory.importance < query.min_importance {
                continue;
            }

            if let Some((start, end)) = query.time_range {
                if memory.created_at < start || memory.created_at > end {
                    continue;
                }
            }

            let text_relevance = self.calculate_text_relevance(&memory.content, &query.query);
            let semantic_similarity =
                self.calculate_cosine_similarity(&query_embedding, &memory.embedding);

            let relevance_score = (text_relevance * 0.3) + (semantic_similarity * 0.7);

            results.push(MemorySearchResult {
                entry: memory,
                relevance_score,
                similarity_score: semantic_similarity,
            });
        }

        results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        results.truncate(query.max_results);

        Ok(results)
    }

    pub async fn consolidate_memories(&self, similarity_threshold: f32) -> Result<usize, AppError> {
        let memories = self.get_stored_memories().await?;
        let mut consolidated_memories: Vec<MemoryEntry> = Vec::new();
        let mut merged_count = 0;

        for memory in memories.iter() {
            let mut should_merge = false;

            for consolidated in &mut consolidated_memories {
                let similarity =
                    self.calculate_cosine_similarity(&memory.embedding, &consolidated.embedding);

                if similarity > similarity_threshold
                    && std::mem::discriminant(&memory.memory_type)
                        == std::mem::discriminant(&consolidated.memory_type)
                {
                    consolidated.content =
                        format!("{}\n\n{}", consolidated.content, memory.content);
                    consolidated.importance = (consolidated.importance + memory.importance) / 2.0;
                    consolidated.access_count += memory.access_count;

                    for (key, value) in &memory.metadata {
                        consolidated.metadata.insert(key.clone(), value.clone());
                    }

                    should_merge = true;
                    merged_count += 1;
                    break;
                }
            }

            if !should_merge {
                consolidated_memories.push(memory.clone());
            }
        }

        self.store_all_memories(&consolidated_memories).await?;

        Ok(merged_count)
    }

    pub async fn forget_memories(
        &self,
        max_memories: usize,
        min_importance: f32,
    ) -> Result<usize, AppError> {
        let mut memories = self.get_stored_memories().await?;
        let initial_count = memories.len();

        memories.retain(|m| m.importance >= min_importance);

        if memories.len() > max_memories {
            memories.sort_by(|a, b| {
                let importance_cmp = b
                    .importance
                    .partial_cmp(&a.importance)
                    .unwrap_or(std::cmp::Ordering::Equal);
                if importance_cmp == std::cmp::Ordering::Equal {
                    b.last_accessed.cmp(&a.last_accessed)
                } else {
                    importance_cmp
                }
            });

            memories.truncate(max_memories);
        }

        let forgotten_count = initial_count - memories.len();

        self.store_all_memories(&memories).await?;

        Ok(forgotten_count)
    }

    pub async fn get_memory_stats(&self) -> Result<Value, AppError> {
        let memories = self.get_stored_memories().await?;

        let mut stats = HashMap::new();
        let mut type_counts = HashMap::new();
        let mut total_importance = 0.0;
        let mut total_access_count = 0;

        for memory in &memories {
            let type_name = format!("{:?}", memory.memory_type);
            *type_counts.entry(type_name).or_insert(0) += 1;
            total_importance += memory.importance;
            total_access_count += memory.access_count;
        }

        stats.insert("total_memories".to_string(), json!(memories.len()));
        stats.insert("memory_types".to_string(), json!(type_counts));
        stats.insert(
            "average_importance".to_string(),
            json!(if memories.is_empty() {
                0.0
            } else {
                total_importance / memories.len() as f32
            }),
        );
        stats.insert("total_access_count".to_string(), json!(total_access_count));

        Ok(json!(stats))
    }

    fn memory_type_matches(&self, memory_type: &MemoryType, query_types: &[MemoryType]) -> bool {
        query_types
            .iter()
            .any(|qt| std::mem::discriminant(memory_type) == std::mem::discriminant(qt))
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

    async fn store_memory_entry(&self, memory: &MemoryEntry) -> Result<(), AppError> {
        StoreMemoryEmbeddingCommand::new(
            memory.id as i64,
            self.deployment_id,
            self.context_id,
            memory.memory_type.to_string(),
            memory.content.clone(),
            memory.embedding.clone(),
            memory.importance,
            memory.access_count as i32,
        )
        .execute(&self.app_state)
        .await?;

        // Also store in database for backup/persistence
        let mut memories = self.get_stored_memories().await?;
        memories.retain(|m| m.id != memory.id);
        memories.push(memory.clone());
        self.store_all_memories(&memories).await
    }

    async fn get_stored_memories(&self) -> Result<Vec<MemoryEntry>, AppError> {
        // Get the latest execution context for this agent
        // let contexts = GetExecutionContextsByAgentQuery::new(
        //     self.context.agent_id,
        //     self.context.deployment_id,
        // )
        // .with_limit(1)
        // .execute(&self.app_state)
        // .await?;

        // if let Some(context) = contexts.first() {
        //     // Deserialize the memory field JSON into Vec<MemoryEntry>
        //     if let Ok(memories) = serde_json::from_value::<Vec<MemoryEntry>>(context.memory.clone())
        //     {
        //         Ok(memories)
        //     } else {
        //         // If deserialization fails, return empty vector
        //         Ok(Vec::new())
        //     }
        // } else {
        //     // No execution context found, return empty vector
        //     Ok(Vec::new())
        // }

        Ok(vec![])
    }

    async fn store_all_memories(&self, _memories: &[MemoryEntry]) -> Result<(), AppError> {
        // use shared::queries::{
        //     GetExecutionContextsByAgentQuery, Query, UpdateExecutionContextQuery,
        // };

        // // Get the latest execution context for this agent
        // let contexts = GetExecutionContextsByAgentQuery::new(
        //     self.context.agent_id,
        //     self.context.deployment_id,
        // )
        // .with_limit(1)
        // .execute(&self.app_state)
        // .await?;

        // if let Some(context) = contexts.first() {
        //     // Serialize the memories to JSON
        //     let memory_json = serde_json::to_value(memories)
        //         .map_err(|e| AppError::Internal(format!("Failed to serialize memories: {}", e)))?;

        //     // Update the execution context memory field in the database
        //     UpdateExecutionContextQuery::new(context.id, self.context.deployment_id)
        //         .with_memory(memory_json)
        //         .execute(&self.app_state)
        //         .await?;

        //     println!(
        //         "Stored {} memories for agent {} in execution context {}",
        //         memories.len(),
        //         self.context.agent_id,
        //         context.id
        //     );
        // } else {
        //     // No execution context found - this shouldn't happen in normal operation
        //     eprintln!(
        //         "Warning: No execution context found for agent {} when storing memories",
        //         self.context.agent_id
        //     );
        // }

        Ok(())
    }
}
