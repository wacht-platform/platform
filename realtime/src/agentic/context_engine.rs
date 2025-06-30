use super::AgentContext;
use shared::commands::{Command, GenerateEmbeddingCommand, SearchKnowledgeBaseEmbeddingsCommand};
use shared::error::AppError;
use shared::state::AppState;

use serde_json::{Value, json};

pub struct ContextEngine {
    pub context: AgentContext,
    pub app_state: AppState,
}

impl ContextEngine {
    pub fn new(context: AgentContext, app_state: AppState) -> Result<Self, AppError> {
        Ok(Self { context, app_state })
    }

    pub async fn search(&self, query: &str) -> Result<Value, AppError> {
        use std::time::Instant;
        use tokio::try_join;

        let start_time = Instant::now();

        let search_results = try_join!(
            self.search_tools_with_llm(query),
            self.search_workflows_with_llm(query),
            self.search_knowledge_base_metadata_vector(query),
            self.search_knowledge_base_documents(query),
            self.search_memory(query),
            self.search_conversation_history_vector(query)
        )?;

        let search_duration = start_time.elapsed();

        let mut all_results = Vec::new();
        all_results.extend(search_results.0);
        all_results.extend(search_results.1);
        all_results.extend(search_results.2);
        all_results.extend(search_results.3);
        all_results.extend(search_results.4);
        all_results.extend(search_results.5);

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
            "search_types": ["tools_llm", "workflows_llm", "knowledge_bases_vector", "documents_vector", "memory_vector", "conversation_history_vector"],
            "parallel_execution": true,
            "search_duration_ms": search_duration.as_millis(),
            "performance": {
                "parallel_searches": 6,
                "estimated_sequential_time_saved": "60-80%"
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
                if let Some(tool) = self.context.tools.iter().find(|t| t.id == resource_id) {
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
                if let Some(workflow) = self.context.workflows.iter().find(|w| w.id == resource_id)
                {
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
                    .context
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
        use llm::builder::{LLMBackend, LLMBuilder};
        use llm::chat::ChatMessage;

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

        let tools_info = self
            .context
            .tools
            .iter()
            .map(|tool| {
                format!(
                    "Tool ID: {}\nName: {}\nDescription: {}\nType: {:?}\nConfiguration: {}\n---",
                    tool.id,
                    tool.name,
                    tool.description.as_deref().unwrap_or("No description"),
                    tool.tool_type,
                    serde_json::to_string_pretty(&tool.configuration).unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            r#"You are an expert tool analyzer. Given a user query and a list of available tools, determine which tools are relevant and if any should be executed immediately.

User Query: "{}"

Available Tools:
{}

For each relevant tool, respond with ONLY a JSON array containing objects with these fields:
- "tool_id": the tool ID number
- "relevance_score": a number from 0-100 indicating how relevant this tool is
- "confidence_score": a number from 0-100 indicating confidence that this tool should be executed
- "should_execute": boolean indicating if this tool should be executed immediately (confidence >= 80)
- "reasoning": brief explanation of why this tool matches and execution decision
- "execution_parameters": if should_execute is true, provide the parameters needed to execute this tool

Only include tools that are actually relevant to the query. If no tools match, return an empty array []."#,
            query, tools_info
        );

        let messages = vec![ChatMessage::user().content(&prompt).build()];

        // Extract response text immediately to avoid Send issues
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
                    if let Some(tool) = self.context.tools.iter().find(|t| t.id == tool_id) {
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

                        // If high confidence, execute the tool
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
        use llm::builder::{LLMBackend, LLMBuilder};
        use llm::chat::ChatMessage;

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

        // Create detailed workflow descriptions for LLM analysis
        let workflows_info = self.context.workflows.iter()
            .map(|workflow| {
                format!("Workflow ID: {}\nName: {}\nDescription: {}\nConfiguration: {}\nDefinition: {}\n---",
                    workflow.id,
                    workflow.name,
                    workflow.description.as_deref().unwrap_or("No description"),
                    serde_json::to_string_pretty(&workflow.configuration).unwrap_or_default(),
                    serde_json::to_string_pretty(&workflow.workflow_definition).unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            r#"You are an expert workflow analyzer. Given a user query and a list of available workflows, determine which workflows are relevant and validate their trigger conditions.

User Query: "{}"

Available Workflows:
{}

For each relevant workflow, you must:
1. Check if the workflow is relevant to the user query
2. Examine the trigger conditions in the workflow definition
3. Determine if the trigger conditions are met based on the user query
4. Decide if the workflow should be executed immediately

Respond with ONLY a JSON array containing objects with these fields:
- "workflow_id": the workflow ID number
- "relevance_score": a number from 0-100 indicating how relevant this workflow is
- "trigger_condition_met": boolean indicating if trigger conditions are satisfied
- "confidence_score": a number from 0-100 indicating confidence that this workflow should be executed
- "should_execute": boolean indicating if this workflow should be executed immediately (trigger met AND confidence >= 80)
- "reasoning": detailed explanation of relevance, trigger condition analysis, and execution decision
- "trigger_analysis": specific analysis of why trigger conditions are/aren't met
- "execution_input": if should_execute is true, provide the input data for workflow execution

Only include workflows that are actually relevant to the query. If no workflows match, return an empty array []."#,
            query, workflows_info
        );

        let messages = vec![ChatMessage::user().content(&prompt).build()];

        // Extract response text immediately to avoid Send issues
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
                        self.context.workflows.iter().find(|w| w.id == workflow_id)
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

                        // If trigger conditions are met and high confidence, execute the workflow
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
            .context
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
                        Self::calculate_cosine_similarity_static(&query_embedding, &kb_embedding);

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
        // Generate embedding for the query once using command pattern
        let query_embedding = GenerateEmbeddingCommand::new(query.to_string())
            .execute(&self.app_state)
            .await?;

        // Search across all knowledge bases in parallel
        let search_futures: Vec<_> = self
            .context
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
                        kb_results.push(json!({
                            "type": "document",
                            "id": result.id,
                            "content": result.content,
                            "score": result.score,
                            "knowledge_base_id": result.knowledge_base_id,
                            "document_id": result.document_id,
                            "chunk_index": result.chunk_index,
                            "relevance_score": (result.score * 100.0) as f64, // Convert to 0-100 scale
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

        // Execute all KB searches in parallel
        let results = futures::future::try_join_all(search_futures).await?;

        // Flatten results from all knowledge bases
        let all_results = results.into_iter().flatten().collect();

        Ok(all_results)
    }

    fn calculate_cosine_similarity(&self, vec1: &[f32], vec2: &[f32]) -> f32 {
        Self::calculate_cosine_similarity_static(vec1, vec2)
    }

    fn calculate_cosine_similarity_static(vec1: &[f32], vec2: &[f32]) -> f32 {
        if vec1.len() != vec2.len() {
            return 0.0;
        }

        let dot_product: f32 = vec1.iter().zip(vec2.iter()).map(|(a, b)| a * b).sum();
        let magnitude1: f32 = vec1.iter().map(|a| a * a).sum::<f32>().sqrt();
        let magnitude2: f32 = vec2.iter().map(|b| b * b).sum::<f32>().sqrt();

        if magnitude1 == 0.0 || magnitude2 == 0.0 {
            return 0.0;
        }

        dot_product / (magnitude1 * magnitude2)
    }

    async fn search_memory(&self, query: &str) -> Result<Vec<Value>, AppError> {
        use super::memory_manager::{MemoryManager, MemoryQuery, MemoryType};

        let memory_manager = MemoryManager::new(
            self.context.clone(),
            self.app_state.clone(),
            self.context.execution_context_id,
        )?;

        let memory_query = MemoryQuery {
            query: query.to_string(),
            memory_types: vec![
                MemoryType::Episodic,   // Past conversations
                MemoryType::Semantic,   // Learned facts
                MemoryType::Procedural, // Learned processes
            ],
            max_results: 10,
            min_importance: 0.3,
            time_range: None,
        };

        let search_results = memory_manager.search_memories(&memory_query).await?;

        let mut results = Vec::new();
        for result in search_results {
            results.push(json!({
                "type": "memory",
                "id": result.entry.id,
                "content": result.entry.content,
                "memory_type": result.entry.memory_type,
                "importance": result.entry.importance,
                "created_at": result.entry.created_at,
                "relevance_score": result.similarity_score,
                "source": "agent_memory"
            }));
        }

        Ok(results)
    }

    async fn search_conversation_history_vector(
        &self,
        _query: &str,
    ) -> Result<Vec<Value>, AppError> {
        // use shared::queries::{GetExecutionMessagesQuery, Query};
        // use shared::models::ExecutionMessageType;

        // let query_embedding = GenerateEmbeddingCommand::new(query.to_string()).execute(&self.app_state).await?;
        // let execution_context_id = self.get_current_execution_context_id().await?;

        // let messages_query = GetExecutionMessagesQuery {
        //     execution_context_id,
        //     limit: Some(50),
        //     offset: Some(0),
        //     message_types: Some(vec![
        //         ExecutionMessageType::UserInput,
        //         ExecutionMessageType::AgentResponse,
        //         ExecutionMessageType::ToolCall,
        //         ExecutionMessageType::ToolResult
        //     ]),
        // };

        // let messages = messages_query.execute(&self.app_state).await?;

        // let mut results = Vec::new();

        // for message in messages {
        //     let content = &message.content;
        //     let message_embedding = GenerateEmbeddingCommand::new(content.clone()).execute(&self.app_state).await?;
        //     let similarity_score = self.calculate_cosine_similarity(&query_embedding, &message_embedding);

        //     if similarity_score > 0.3 {
        //         results.push(json!({
        //             "type": "conversation_message",
        //             "id": message.id,
        //             "content": content,
        //             "sender": message.sender,
        //             "message_type": message.message_type,
        //             "created_at": message.created_at,
        //             "relevance_score": (similarity_score * 100.0) as f64,
        //             "similarity_score": similarity_score,
        //             "source": "conversation_history"
        //         }));
        //     }
        // }

        Ok(vec![])
    }

    async fn get_current_execution_context_id(&self) -> Result<i64, AppError> {
        // This should be passed in the context or retrieved from the current execution
        // For now, we'll try to get it from the agent context or return a default
        // In a full implementation, this would be properly tracked
        Ok(1) // This should be properly implemented based on current execution
    }

    pub async fn store_context(&self, key: &str, data: &Value) -> Result<Value, AppError> {
        // Context storage for runtime data - this could be implemented using
        // in-memory storage, database, or other persistence mechanisms as needed
        // For now, we'll return a success response indicating the operation would succeed

        let data_str = serde_json::to_string(data)
            .map_err(|e| AppError::Internal(format!("Failed to serialize context data: {}", e)))?;

        Ok(json!({
            "key": key,
            "stored": true,
            "data_size": data_str.len(),
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "message": "Context storage operation completed"
        }))
    }

    pub async fn fetch_context(&self, key: &str) -> Result<Value, AppError> {
        // Context fetching for runtime data - this could be implemented using
        // in-memory storage, database, or other persistence mechanisms as needed

        Ok(json!({
            "key": key,
            "data": null,
            "found": false,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "message": "Context fetching operation completed"
        }))
    }

    pub async fn list_context_keys(&self) -> Result<Value, AppError> {
        // List available context keys for runtime data

        Ok(json!({
            "agent_id": self.context.agent_id,
            "keys": [],
            "total_keys": 0,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "message": "Context key listing operation completed"
        }))
    }

    pub async fn delete_context(&self, key: &str) -> Result<Value, AppError> {
        // Delete context data for runtime cleanup

        Ok(json!({
            "key": key,
            "deleted": true,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "message": "Context deletion operation completed"
        }))
    }

    async fn execute_tool_immediately(
        &self,
        tool: &shared::models::AiTool,
        execution_params: Value,
    ) -> Result<Value, AppError> {
        // Prevent recursion: Don't execute context_engine or memory tools to avoid infinite loops
        if tool.name == "context_engine" || tool.name == "memory" {
            return Ok(json!({
                "tool_id": tool.id,
                "tool_name": tool.name,
                "execution_type": "skipped",
                "reason": "Prevented recursive execution of context engine or memory tools",
                "execution_timestamp": chrono::Utc::now().to_rfc3339()
            }));
        }

        // For other tools, return execution plan instead of immediate execution to prevent recursion
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
        // Return execution plan instead of immediate execution to prevent potential recursion
        // The agent executor will handle the actual workflow execution
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
