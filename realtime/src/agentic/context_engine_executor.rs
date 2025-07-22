use crate::agentic::{
    ContextSearchDerivation, KnowledgeBaseSearchExecution, KnowledgeBaseSearchPlan,
    KnowledgeBaseSearchValidation, SearchLoopDecision, SearchScope, SearchStrategy,
    gemini_client::GeminiClient,
};
use crate::template::{AgentTemplates, render_template_with_prompt};
use chrono::Duration;
use serde_json::{Value, json};
use shared::commands::{Command, GenerateEmbeddingCommand, SearchKnowledgeBaseEmbeddingsCommand};
use shared::error::AppError;
use shared::models::{
    AiAgentWithFeatures, AiKnowledgeBaseDocument, ContextAction, ContextEngineParams,
    ContextFilters, ContextSearchResult, ContextSource, ConversationContent, ConversationRecord,
    SearchMode, TimeRange,
};
use shared::queries::{
    DebugKnowledgeBaseContentQuery, DebugTextSearchQuery, FullTextSearchKnowledgeBaseQuery,
    GetKnowledgeBaseDocumentsQuery, HybridSearchKnowledgeBaseQuery, Query, SearchMemoriesQuery,
};
use shared::state::AppState;
use std::collections::HashSet;

const DEDUPLICATION_THRESHOLD: f32 = 0.9;
const RELEVANCE_SCORE_DIVISOR: f32 = 2.0;

#[derive(Clone)]
pub struct ContextEngineExecutor {
    app_state: AppState,
    context_id: i64,
    agent: AiAgentWithFeatures,
}

impl ContextEngineExecutor {
    pub fn new(app_state: AppState, context_id: i64, agent: AiAgentWithFeatures) -> Self {
        Self {
            app_state,
            context_id,
            agent,
        }
    }

    pub async fn execute(
        &self,
        params: ContextEngineParams,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let query_embedding = self.generate_embedding(&params.query).await?;

        println!("{params:?}");

        let filters = params.filters.unwrap_or_default();

        match params.action {
            ContextAction::SearchKnowledgeBase { kb_id } => {
                self.search_knowledge_bases(&params.query, &query_embedding, kb_id, &filters)
                    .await
            }
            ContextAction::SearchDynamicContext { context_type } => {
                self.search_dynamic_context(&params.query, &query_embedding, context_type, &filters)
                    .await
            }
            ContextAction::SearchMemories { category } => {
                self.search_memories(&params.query, &query_embedding, category, &filters)
                    .await
            }
            ContextAction::SearchConversations { context_id } => {
                let search_context_id = context_id.unwrap_or(self.context_id);
                self.search_conversations(
                    &params.query,
                    &query_embedding,
                    search_context_id,
                    &filters,
                )
                .await
            }
            ContextAction::SearchAll => {
                self.search_all(&params.query, &query_embedding, &filters)
                    .await
            }
        }
    }

    // ==================== Core Search Methods ====================

    async fn search_knowledge_bases(
        &self,
        query: &str,
        query_embedding: &[f32],
        kb_id: Option<i64>,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        tracing::info!(
            query = query,
            kb_id = ?kb_id,
            max_results = filters.max_results,
            min_relevance = filters.min_relevance,
            search_mode = ?filters.search_mode,
            "Starting knowledge base search"
        );

        match kb_id {
            Some(id) => {
                self.search_single_kb(id, query, query_embedding, filters)
                    .await
            }
            None => {
                self.search_all_agent_kbs(query, query_embedding, filters)
                    .await
            }
        }
    }

    async fn search_memories(
        &self,
        _query: &str,
        query_embedding: &[f32],
        category: Option<String>,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let memory_type_filter = category.map(|cat| vec![cat]).unwrap_or_else(|| {
            vec!["procedural", "semantic", "episodic"]
                .into_iter()
                .map(String::from)
                .collect()
        });

        let search_results = SearchMemoriesQuery {
            agent_id: self.agent.id,
            query_embedding: query_embedding.to_vec(),
            limit: filters.max_results as i64,
            memory_type_filter,
            min_importance: Some(filters.min_relevance),
            time_range: filters.time_range.as_ref().map(|tr| (tr.start, tr.end)),
        }
        .execute(&self.app_state)
        .await?;

        Ok(search_results
            .into_iter()
            .filter_map(|record| {
                let relevance = self.calculate_relevance_score(record.score);
                (relevance >= filters.min_relevance).then(|| ContextSearchResult {
                    source: ContextSource::Memory {
                        memory_id: record.id,
                        category: record.memory_type.clone(),
                    },
                    content: record.content,
                    relevance_score: relevance,
                    metadata: json!({
                        "importance": record.importance,
                        "access_count": record.access_count,
                    }),
                })
            })
            .collect())
    }

    async fn search_dynamic_context(
        &self,
        _query: &str,
        query_embedding: &[f32],
        _context_type: Option<String>,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        use shared::queries::agent_dynamic_context::SearchAgentDynamicContextQuery;

        let search_results = SearchAgentDynamicContextQuery {
            execution_context_id: self.context_id,
            query_embedding: query_embedding.to_vec(),
            limit: filters.max_results as i64,
        }
        .execute(&self.app_state)
        .await?;

        Ok(search_results
            .into_iter()
            .filter_map(|record| {
                let relevance = self.calculate_relevance_score(record.score);
                (relevance >= filters.min_relevance).then(|| {
                    let source_type = record.source.unwrap_or_else(|| "unknown".to_string());
                    ContextSearchResult {
                        source: ContextSource::DynamicContext {
                            context_type: source_type.clone(),
                        },
                        content: record.content,
                        relevance_score: relevance,
                        metadata: json!({
                            "context_id": record.execution_context_id,
                            "source": source_type,
                            "created_at": record.created_at,
                        }),
                    }
                })
            })
            .collect())
    }

    async fn search_conversations(
        &self,
        _query: &str,
        _query_embedding: &[f32],
        _context_id: i64,
        _filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        // TODO: Implement conversation search
        Ok(Vec::new())
    }

    async fn search_all(
        &self,
        query: &str,
        query_embedding: &[f32],
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        // Execute all searches concurrently
        let (memories_result, kb_result, dynamic_result, conv_result) = tokio::join!(
            self.search_memories(query, query_embedding, None, filters),
            self.search_knowledge_bases(query, query_embedding, None, filters),
            self.search_dynamic_context(query, query_embedding, None, filters),
            self.search_conversations(query, query_embedding, self.context_id, filters)
        );

        let search_results = vec![memories_result, kb_result, dynamic_result, conv_result];

        // Collect successful results, log errors
        let mut all_results = Vec::new();
        for (idx, result) in search_results.into_iter().enumerate() {
            match result {
                Ok(results) => all_results.extend(results),
                Err(e) => {
                    let source_name = match idx {
                        0 => "memories",
                        1 => "knowledge bases",
                        2 => "dynamic context",
                        3 => "conversations",
                        _ => "unknown",
                    };
                    tracing::warn!(source = source_name, error = %e, "Search failed");
                }
            }
        }

        // Post-process results
        let deduplicated = self.deduplicate_results(all_results);
        let final_results = self.sort_and_limit_results(deduplicated, filters.max_results);

        tracing::info!(
            total_results = final_results.len(),
            "Completed search across all sources"
        );

        Ok(final_results)
    }

    // ==================== Helper Methods ====================

    async fn generate_embedding(&self, query: &str) -> Result<Vec<f32>, AppError> {
        GenerateEmbeddingCommand::new(query.to_string())
            .execute(&self.app_state)
            .await
    }

    async fn search_single_kb(
        &self,
        kb_id: i64,
        query: &str,
        query_embedding: &[f32],
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        tracing::info!(
            kb_id = kb_id,
            search_mode = ?filters.search_mode,
            "Searching single knowledge base"
        );

        match &filters.search_mode {
            SearchMode::Vector => {
                // Pure vector search (existing implementation)
                let search_results = SearchKnowledgeBaseEmbeddingsCommand::new(
                    kb_id,
                    query_embedding.to_vec(),
                    filters.max_results as u64,
                )
                .execute(&self.app_state)
                .await?;

                let results: Vec<_> = search_results
                    .into_iter()
                    .enumerate()
                    .filter_map(|(idx, result)| {
                        let relevance = self.calculate_relevance_score(result.score);

                        tracing::debug!(
                            result_idx = idx,
                            score = result.score,
                            relevance = relevance,
                            doc_id = result.document_id,
                            "Processing search result"
                        );

                        (relevance >= filters.min_relevance)
                            .then(|| self.create_kb_search_result(kb_id, result, relevance))
                    })
                    .collect();

                Ok(results)
            }
            SearchMode::FullText => {
                // Pure full-text search
                self.search_kb_fulltext(kb_id, query, filters).await
            }
            SearchMode::Hybrid {
                vector_weight,
                text_weight,
            } => {
                // Hybrid search combining vector and full-text
                self.search_kb_hybrid(
                    kb_id,
                    query,
                    query_embedding,
                    filters,
                    *vector_weight,
                    *text_weight,
                )
                .await
            }
        }
    }

    async fn search_all_agent_kbs(
        &self,
        query: &str,
        query_embedding: &[f32],
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let kb_ids = self.get_agent_knowledge_base_ids();

        if kb_ids.is_empty() {
            tracing::warn!(agent_id = self.agent.id, "Agent has no knowledge bases");
            return Ok(Vec::new());
        }

        tracing::info!(
            agent_id = self.agent.id,
            kb_count = kb_ids.len(),
            kb_ids = ?kb_ids,
            "Searching multiple knowledge bases"
        );

        let mut all_results = Vec::new();

        // Search each KB and collect results
        for kb_id in kb_ids {
            match self
                .search_single_kb(kb_id, query, query_embedding, filters)
                .await
            {
                Ok(results) => {
                    tracing::info!(kb_id = kb_id, count = results.len(), "KB search succeeded");
                    all_results.extend(results);
                }
                Err(e) => {
                    tracing::error!(kb_id = kb_id, error = %e, "KB search failed");
                    // Continue with other KBs
                }
            }
        }

        // Sort and limit results
        Ok(self.sort_and_limit_results(all_results, filters.max_results))
    }

    fn get_agent_knowledge_base_ids(&self) -> Vec<i64> {
        self.agent.knowledge_bases.iter().map(|kb| kb.id).collect()
    }

    fn create_kb_search_result(
        &self,
        kb_id: i64,
        result: shared::models::DocumentChunkSearchResult,
        relevance: f64,
    ) -> ContextSearchResult {
        ContextSearchResult {
            source: ContextSource::KnowledgeBase {
                kb_id,
                document_id: result.document_id,
            },
            content: result.content,
            relevance_score: relevance,
            metadata: json!({
                "chunk_index": result.chunk_index,
                "document_id": result.document_id,
            }),
        }
    }

    fn calculate_relevance_score(&self, distance: f64) -> f64 {
        (1.0 - (distance / RELEVANCE_SCORE_DIVISOR as f64)).max(0.0)
    }

    fn deduplicate_results(&self, results: Vec<ContextSearchResult>) -> Vec<ContextSearchResult> {
        let mut deduplicated = Vec::with_capacity(results.len());

        for result in results {
            if let Some(existing_idx) =
                deduplicated
                    .iter()
                    .position(|existing: &ContextSearchResult| {
                        self.are_similar(&result.content, &existing.content)
                    })
            {
                // Keep the one with higher relevance
                if result.relevance_score > deduplicated[existing_idx].relevance_score {
                    deduplicated[existing_idx] = result;
                }
            } else {
                deduplicated.push(result);
            }
        }

        deduplicated
    }

    fn are_similar(&self, text1: &str, text2: &str) -> bool {
        self.calculate_text_similarity(text1, text2) > DEDUPLICATION_THRESHOLD
    }

    fn calculate_text_similarity(&self, text1: &str, text2: &str) -> f32 {
        let words1: HashSet<&str> = text1.split_whitespace().collect();
        let words2: HashSet<&str> = text2.split_whitespace().collect();

        if words1.is_empty() && words2.is_empty() {
            return 1.0;
        }

        let intersection_count = words1.intersection(&words2).count();
        let union_count = words1.union(&words2).count();

        if union_count == 0 {
            0.0
        } else {
            intersection_count as f32 / union_count as f32
        }
    }

    fn sort_and_limit_results(
        &self,
        mut results: Vec<ContextSearchResult>,
        max_results: usize,
    ) -> Vec<ContextSearchResult> {
        // Sort by relevance score (highest first)
        results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Limit to max results
        results.truncate(max_results);
        results
    }

    // ==================== Hybrid Search Methods ====================

    async fn search_kb_fulltext(
        &self,
        kb_id: i64,
        query: &str,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let query = FullTextSearchKnowledgeBaseQuery {
            query_text: query.to_string(),
            knowledge_base_id: kb_id,
            deployment_id: self.agent.deployment_id,
            max_results: filters.max_results as i32,
        };

        let results = query.execute(&self.app_state).await?;

        Ok(results
            .into_iter()
            .filter_map(|result| {
                let relevance = (result.text_rank * 10.0).min(1.0);
                (relevance >= filters.min_relevance).then(|| ContextSearchResult {
                    source: ContextSource::KnowledgeBase {
                        kb_id,
                        document_id: result.document_id,
                    },
                    content: result.content,
                    relevance_score: relevance,
                    metadata: json!({
                        "chunk_index": result.chunk_index,
                        "document_id": result.document_id,
                        "document_title": result.document_title,
                        "document_description": result.document_description,
                        "search_mode": "full_text",
                        "text_rank": result.text_rank,
                    }),
                })
            })
            .collect())
    }

    async fn search_kb_hybrid(
        &self,
        kb_id: i64,
        query: &str,
        query_embedding: &[f32],
        filters: &ContextFilters,
        vector_weight: f32,
        text_weight: f32,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        // Add debug logging for the search parameters
        tracing::info!(
            "Starting hybrid search for KB {} with query: '{}', deployment: {}, min_relevance: {}, max_results: {}",
            kb_id,
            query,
            self.agent.deployment_id,
            filters.min_relevance,
            filters.max_results
        );

        // Debug: Check KB contents
        let debug_query = DebugKnowledgeBaseContentQuery {
            knowledge_base_id: kb_id,
            deployment_id: self.agent.deployment_id,
        };

        match debug_query.execute(&self.app_state).await {
            Ok(debug_info) => {
                tracing::info!(
                    "KB {} debug info - Total chunks: {}, With vectors: {}, Without vectors: {}",
                    kb_id,
                    debug_info.total_chunks,
                    debug_info.chunks_with_vectors,
                    debug_info.chunks_without_vectors
                );

                for (idx, content) in debug_info.sample_content.iter().enumerate() {
                    tracing::debug!(
                        "Sample content {}: '{}'",
                        idx,
                        &content[..content.len().min(100)]
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Failed to get KB debug info: {}", e);
            }
        }

        // Debug: Check text search
        let text_debug_query = DebugTextSearchQuery {
            knowledge_base_id: kb_id,
            deployment_id: self.agent.deployment_id,
            search_term: "Niroj".to_string(),
        };

        match text_debug_query.execute(&self.app_state).await {
            Ok(text_debug) => {
                tracing::info!(
                    "Text search for 'Niroj' in KB {} found {} matching chunks",
                    kb_id,
                    text_debug.matching_chunks
                );

                for (idx, content) in text_debug.sample_matches.iter().enumerate() {
                    tracing::debug!(
                        "Niroj match {}: '{}'",
                        idx,
                        &content[..content.len().min(200)]
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Failed to run text search debug: {}", e);
            }
        }

        // Log embedding info
        tracing::info!(
            "Query embedding length: {}, first few values: [{:.4}, {:.4}, {:.4}...]",
            query_embedding.len(),
            query_embedding.get(0).unwrap_or(&0.0),
            query_embedding.get(1).unwrap_or(&0.0),
            query_embedding.get(2).unwrap_or(&0.0)
        );

        let query = HybridSearchKnowledgeBaseQuery {
            query_text: query.to_string(),
            query_embedding: query_embedding.to_vec(),
            knowledge_base_id: kb_id,
            deployment_id: self.agent.deployment_id,
            max_results: filters.max_results as i32,
            min_relevance: filters.min_relevance,
            vector_weight: vector_weight as f64,
            text_weight: text_weight as f64,
        };

        let results = query.execute(&self.app_state).await?;

        Ok(results
            .into_iter()
            .map(|result| ContextSearchResult {
                source: ContextSource::KnowledgeBase {
                    kb_id,
                    document_id: result.document_id,
                },
                content: result.content,
                relevance_score: result.combined_score,
                metadata: json!({
                    "chunk_index": result.chunk_index,
                    "document_id": result.document_id,
                    "document_title": result.document_title,
                    "document_description": result.document_description,
                    "vector_similarity": result.vector_similarity,
                    "text_rank": result.text_rank,
                    "combined_score": result.combined_score,
                    "search_mode": "hybrid",
                    "weights": {
                        "vector": vector_weight,
                        "text": text_weight
                    }
                }),
            })
            .collect())
    }
}

pub struct ContextGatheringOrchestrator {
    context_engine: ContextEngineExecutor,
    app_state: AppState,
    agent: AiAgentWithFeatures,
}

impl ContextGatheringOrchestrator {
    pub fn new(app_state: AppState, context_id: i64, agent: AiAgentWithFeatures) -> Self {
        let context_engine =
            ContextEngineExecutor::new(app_state.clone(), context_id, agent.clone());

        Self {
            context_engine,
            app_state,
            agent,
        }
    }

    pub async fn gather_context(
        &self,
        conversation_history: &[ConversationRecord],
        current_objective: &Option<String>,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        // Derive search parameters using LLM
        let search_params = self
            .derive_context_search_parameters(conversation_history, current_objective)
            .await?;

        // Execute search based on scope
        let context_results = match search_params.search_scope {
            SearchScope::KnowledgeBase => {
                self.iterative_knowledge_base_search(&search_params).await?
            }
            SearchScope::Experience => self.search_experience_with_filters(&search_params).await?,
            SearchScope::Universal => {
                let mut results = Vec::new();

                // Use iterative search for knowledge base portion
                let kb_results = self.iterative_knowledge_base_search(&search_params).await?;
                results.extend(kb_results);

                let exp_results = self.search_experience_with_filters(&search_params).await?;
                results.extend(exp_results);

                results
            }
        };

        Ok(context_results)
    }

    /// Execute a search action from task execution
    pub async fn execute_search_action(
        &self,
        search_type: &str,
        query: &str,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        match search_type {
            "knowledge_base" => self.search_knowledge_bases(query).await,
            "all_sources" => self.search_context(query).await,
            _ => Err(AppError::BadRequest(format!(
                "Unknown search type: {}",
                search_type
            ))),
        }
    }

    async fn search_context(&self, query: &str) -> Result<Vec<ContextSearchResult>, AppError> {
        let params = ContextEngineParams {
            query: query.to_string(),
            action: ContextAction::SearchAll,
            filters: None,
        };

        self.context_engine.execute(params).await
    }

    async fn search_knowledge_bases(
        &self,
        query: &str,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let params = ContextEngineParams {
            query: query.to_string(),
            action: ContextAction::SearchKnowledgeBase { kb_id: None },
            filters: None,
        };

        self.context_engine.execute(params).await
    }

    async fn derive_context_search_parameters(
        &self,
        conversation_history: &[ConversationRecord],
        current_objective: &Option<String>,
    ) -> Result<ContextSearchDerivation, AppError> {
        let context = json!({
            "conversation_history": self.format_conversation_history(conversation_history),
            "current_objective": current_objective,
        });

        let request_body =
            render_template_with_prompt(AgentTemplates::CONTEXT_SEARCH_DERIVATION, &context)
                .map_err(|e| {
                    AppError::Internal(format!(
                        "Failed to render context search derivation template: {}",
                        e
                    ))
                })?;

        let search_params = self
            .create_weak_llm()?
            .generate_structured_content::<ContextSearchDerivation>(request_body)
            .await?;

        Ok(search_params)
    }

    fn format_conversation_history(
        &self,
        conversation_history: &[ConversationRecord],
    ) -> Vec<Value> {
        conversation_history
            .iter()
            .map(|conv| {
                json!({
                    "role": conv.message_type,
                    "content": self.extract_conversation_content(&conv.content),
                    "timestamp": conv.created_at,
                })
            })
            .collect()
    }

    fn create_weak_llm(&self) -> Result<GeminiClient, AppError> {
        // TODO: Get API key from configuration
        let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_else(|_| "test-key".to_string());
        Ok(GeminiClient::new(
            api_key,
            Some("gemini-1.5-flash".to_string()),
        ))
    }

    async fn iterative_knowledge_base_search(
        &self,
        initial_params: &ContextSearchDerivation,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        const MAX_ITERATIONS: usize = 5;
        let mut all_results = Vec::new();
        let mut current_params = initial_params.clone();
        let mut previous_attempts = Vec::new();

        for iteration in 0..MAX_ITERATIONS {
            // Step 1: Plan the search strategy
            let search_plan = self
                .plan_knowledge_base_search(&current_params, &all_results, iteration)
                .await?;

            // Step 2: Execute the search plan
            let (execution_report, iteration_results) = self
                .execute_knowledge_base_search_plan(&search_plan, &current_params, iteration)
                .await?;

            // Track this attempt
            previous_attempts.push(execution_report.clone());

            // Add any results found
            if !iteration_results.is_empty() {
                all_results.extend(iteration_results);
            }

            // Step 3: Validate the results and decide next action
            let validation = self
                .validate_knowledge_base_search(
                    &current_params,
                    &search_plan,
                    &execution_report,
                    &all_results,
                )
                .await?;

            // Handle the loop decision
            match validation.loop_decision {
                SearchLoopDecision::Complete => {
                    // Success! Return the results
                    break;
                }
                SearchLoopDecision::RefineAndRetry => {
                    // Refine parameters based on validation feedback
                    if let Some(guidance) = &validation.next_iteration_guidance {
                        current_params = self
                            .refine_params_from_guidance(&current_params, guidance, &validation)
                            .await?;
                    }
                }
                SearchLoopDecision::TryAlternativeStrategy => {
                    // Switch to a different approach
                    if !current_params.alternative_queries.is_empty() {
                        current_params.search_query = current_params.alternative_queries.remove(0);
                    } else {
                        // No alternatives left, use validation guidance
                        if let Some(guidance) = &validation.next_iteration_guidance {
                            current_params = self
                                .refine_params_from_guidance(&current_params, guidance, &validation)
                                .await?;
                        }
                    }
                }
                SearchLoopDecision::AbortInsufficient => {
                    // Knowledge base doesn't have the information
                    eprintln!("Knowledge base search aborted - insufficient information");
                    break;
                }
            }

            // Prevent infinite loops
            if iteration >= MAX_ITERATIONS - 1 {
                eprintln!("Reached maximum search iterations");
                break;
            }
        }

        Ok(all_results)
    }

    async fn plan_knowledge_base_search(
        &self,
        search_params: &ContextSearchDerivation,
        previous_results: &[ContextSearchResult],
        iteration: usize,
    ) -> Result<KnowledgeBaseSearchPlan, AppError> {
        let context = json!({
            "search_query": search_params.search_query,
            "search_scope": format!("{:?}", search_params.search_scope),
            "available_knowledge_bases": self.agent.knowledge_bases.iter().map(|kb| kb.id).collect::<Vec<i64>>(),
            "conversation_history": self.format_conversation_history(&[]), // TODO: pass actual history
            "current_objective": None::<String>, // TODO: pass actual objective
            "iteration_number": iteration + 1,
            "previous_results_count": previous_results.len(),
            "previous_results_summary": self.summarize_results(previous_results),
        });

        let request_body = render_template_with_prompt(AgentTemplates::KB_SEARCH_PLAN, &context)
            .map_err(|e| {
                AppError::Internal(format!("Failed to render KB search plan template: {}", e))
            })?;

        let plan = self
            .create_weak_llm()?
            .generate_structured_content::<KnowledgeBaseSearchPlan>(request_body)
            .await?;

        Ok(plan)
    }

    async fn execute_knowledge_base_search_plan(
        &self,
        search_plan: &KnowledgeBaseSearchPlan,
        search_params: &ContextSearchDerivation,
        iteration: usize,
    ) -> Result<(KnowledgeBaseSearchExecution, Vec<ContextSearchResult>), AppError> {
        let start_time = std::time::Instant::now();
        let mut all_results = Vec::new();
        let mut documents_scanned = 0;
        let mut chunks_analyzed = 0;
        let mut challenges = Vec::new();

        // Try primary strategy first
        let strategy_result = self
            .execute_single_strategy(&search_plan.primary_strategy, search_params)
            .await;

        match strategy_result {
            Ok((results, doc_count, chunk_count)) => {
                all_results.extend(results);
                documents_scanned += doc_count;
                chunks_analyzed += chunk_count;
            }
            Err(e) => {
                challenges.push(format!("Primary strategy failed: {}", e));

                // Try fallback strategies
                for fallback in &search_plan.fallback_strategies {
                    match self.execute_single_strategy(fallback, search_params).await {
                        Ok((results, doc_count, chunk_count)) => {
                            all_results.extend(results);
                            documents_scanned += doc_count;
                            chunks_analyzed += chunk_count;
                            break; // Success with fallback
                        }
                        Err(e) => {
                            challenges.push(format!(
                                "Fallback '{}' failed: {}",
                                fallback.strategy_type, e
                            ));
                        }
                    }
                }
            }
        }

        let execution_time = start_time.elapsed().as_millis() as i64;

        // Generate execution report using LLM for intelligent analysis
        let execution_context = json!({
            "search_plan": search_plan,
            "current_strategy": &search_plan.primary_strategy,
            "iteration_number": iteration + 1,
            "previous_attempts": Vec::<String>::new(), // TODO: pass actual previous attempts
            "time_budget_ms": 30000,
            "documents_found": documents_scanned,
            "chunks_analyzed": chunks_analyzed,
            "current_results_summary": self.summarize_results(&all_results),
        });

        let request_body =
            render_template_with_prompt(AgentTemplates::KB_SEARCH_EXECUTION, &execution_context)
                .map_err(|e| {
                    AppError::Internal(format!(
                        "Failed to render KB search execution template: {}",
                        e
                    ))
                })?;

        let mut execution_report = self
            .create_weak_llm()?
            .generate_structured_content::<KnowledgeBaseSearchExecution>(request_body)
            .await?;

        // Override with actual values
        execution_report.results_found = all_results.len() as i32;
        execution_report.execution_details.documents_scanned = documents_scanned;
        execution_report.execution_details.chunks_analyzed = chunks_analyzed;
        execution_report.execution_details.time_taken_ms = execution_time;
        execution_report.execution_details.challenges_encountered = challenges;

        Ok((execution_report, all_results))
    }

    async fn validate_knowledge_base_search(
        &self,
        search_params: &ContextSearchDerivation,
        search_plan: &KnowledgeBaseSearchPlan,
        execution_report: &KnowledgeBaseSearchExecution,
        all_results: &[ContextSearchResult],
    ) -> Result<KnowledgeBaseSearchValidation, AppError> {
        let validation_context = json!({
            "original_query": search_params.search_query,
            "search_objective": None::<String>, // TODO: pass actual objective
            "success_criteria": search_plan.success_criteria,
            "search_plan": search_plan,
            "execution_report": execution_report,
            "total_results": all_results.len(),
            "results_summary": self.summarize_results(all_results),
            "result_samples": self.sample_results(all_results, 3),
        });

        let request_body =
            render_template_with_prompt(AgentTemplates::KB_SEARCH_VALIDATION, &validation_context)
                .map_err(|e| {
                    AppError::Internal(format!(
                        "Failed to render KB search validation template: {}",
                        e
                    ))
                })?;

        let validation = self
            .create_weak_llm()?
            .generate_structured_content::<KnowledgeBaseSearchValidation>(request_body)
            .await?;

        Ok(validation)
    }

    async fn execute_single_strategy(
        &self,
        strategy: &SearchStrategy,
        search_params: &ContextSearchDerivation,
    ) -> Result<(Vec<ContextSearchResult>, i32, i32), AppError> {
        let mut results = Vec::new();
        let mut docs_scanned = 0;
        let mut chunks_analyzed = 0;

        match strategy.strategy_type.as_str() {
            "document_discovery" | "list_documents" => {
                let documents = self
                    .list_knowledge_base_documents(
                        strategy.parameters.knowledge_base_id,
                        strategy.parameters.document_keyword.as_deref(),
                    )
                    .await?;

                docs_scanned = documents.len() as i32;

                for doc in documents {
                    let content = format!(
                        "Document: {}\nDescription: {}\nType: {}\nSize: {} bytes",
                        doc.title,
                        doc.description.as_deref().unwrap_or("No description"),
                        doc.file_type,
                        doc.file_size
                    );

                    results.push(ContextSearchResult {
                        content,
                        source: ContextSource::KnowledgeBase {
                            kb_id: doc.knowledge_base_id,
                            document_id: doc.id,
                        },
                        relevance_score: 1.0,
                        metadata: json!({
                            "document_id": doc.id,
                            "title": doc.title,
                            "file_name": doc.file_name,
                            "file_type": doc.file_type,
                        }),
                    });
                }
            }
            "keyword_search" | "keyword_document_search" => {
                if let Some(keywords) = &strategy.parameters.keywords {
                    let params = ContextEngineParams {
                        query: keywords.join(" "),
                        action: ContextAction::SearchKnowledgeBase {
                            kb_id: strategy.parameters.knowledge_base_id,
                        },
                        filters: Some(ContextFilters {
                            max_results: strategy.parameters.max_chunks.unwrap_or(20) as usize,
                            min_relevance: strategy.parameters.similarity_threshold.unwrap_or(0.7),
                            time_range: None,
                            search_mode: SearchMode::FullText,
                            boost_keywords: strategy.parameters.keyword_boost.clone(),
                        }),
                    };

                    let search_results = self.context_engine.execute(params).await?;

                    chunks_analyzed = search_results.len() as i32;
                    results.extend(search_results);
                }
            }
            "semantic_search" | "chunk_search" => {
                let query = strategy
                    .parameters
                    .search_query
                    .clone()
                    .unwrap_or_else(|| search_params.search_query.clone());

                let params = ContextEngineParams {
                    query,
                    action: ContextAction::SearchKnowledgeBase {
                        kb_id: strategy.parameters.knowledge_base_id,
                    },
                    filters: Some(ContextFilters {
                        max_results: strategy.parameters.max_chunks.unwrap_or(20) as usize,
                        min_relevance: strategy.parameters.similarity_threshold.unwrap_or(0.7),
                        time_range: None,
                        search_mode: SearchMode::Vector,
                        boost_keywords: strategy.parameters.keyword_boost.clone(),
                    }),
                };

                let search_results = self.context_engine.execute(params).await?;

                chunks_analyzed = search_results.len() as i32;
                results.extend(search_results);
            }
            _ => {
                return Err(AppError::BadRequest(format!(
                    "Unknown strategy type: {}",
                    strategy.strategy_type
                )));
            }
        }

        Ok((results, docs_scanned, chunks_analyzed))
    }

    async fn list_knowledge_base_documents(
        &self,
        kb_id: Option<i64>,
        keyword_filter: Option<&str>,
    ) -> Result<Vec<AiKnowledgeBaseDocument>, AppError> {
        let mut all_documents = Vec::new();

        // Determine which knowledge bases to query
        let kb_ids_to_query: Vec<i64> = if let Some(specific_kb_id) = kb_id {
            // Check if agent has access to this knowledge base
            if self
                .agent
                .knowledge_bases
                .iter()
                .any(|kb| kb.id == specific_kb_id)
            {
                vec![specific_kb_id]
            } else {
                return Err(AppError::BadRequest(format!(
                    "Agent does not have access to knowledge base {}",
                    specific_kb_id
                )));
            }
        } else {
            // Query all knowledge bases the agent has access to
            self.agent.knowledge_bases.iter().map(|kb| kb.id).collect()
        };

        // List documents for selected knowledge bases
        for kb_id in kb_ids_to_query {
            let query = GetKnowledgeBaseDocumentsQuery::new(kb_id, 100, 0);
            match query.execute(&self.app_state).await {
                Ok(mut documents) => {
                    // Apply keyword filter if provided
                    if let Some(keyword) = keyword_filter {
                        let keyword_lower = keyword.to_lowercase();
                        documents.retain(|doc| {
                            doc.title.to_lowercase().contains(&keyword_lower)
                                || doc
                                    .description
                                    .as_ref()
                                    .map(|desc| desc.to_lowercase().contains(&keyword_lower))
                                    .unwrap_or(false)
                                || doc.file_name.to_lowercase().contains(&keyword_lower)
                        });
                    }
                    all_documents.extend(documents);
                }
                Err(e) => {
                    // Log error but continue with other knowledge bases
                    eprintln!(
                        "Failed to list documents for knowledge base {}: {}",
                        kb_id, e
                    );
                }
            }
        }

        Ok(all_documents)
    }

    async fn search_experience_with_filters(
        &self,
        search_params: &ContextSearchDerivation,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        // Search memories only for experience scope
        self.search_memories_with_filters(search_params).await
    }

    async fn search_memories_with_filters(
        &self,
        search_params: &ContextSearchDerivation,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let params = ContextEngineParams {
            query: search_params.search_query.clone(),
            action: ContextAction::SearchMemories { category: None },
            filters: Some(ContextFilters {
                max_results: search_params.filters.max_results as usize,
                min_relevance: search_params.filters.min_relevance,
                time_range: self.parse_time_range(&search_params.filters.time_range),
                search_mode: match &search_params.filters.search_mode {
                    crate::agentic::SearchModeType::Semantic => SearchMode::Vector,
                    crate::agentic::SearchModeType::Keyword => SearchMode::FullText,
                    crate::agentic::SearchModeType::Hybrid => SearchMode::Hybrid {
                        vector_weight: 0.5,
                        text_weight: 0.5,
                    },
                },
                boost_keywords: search_params.filters.boost_keywords.clone(),
            }),
        };

        self.context_engine.execute(params).await
    }

    fn parse_time_range(&self, time_range: &Option<String>) -> Option<TimeRange> {
        use chrono::{TimeZone, Utc};

        time_range.as_ref().and_then(|range| {
            let now = Utc::now();
            let start = match range.as_str() {
                "last_hour" => now - Duration::hours(1),
                "today" => now
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .and_then(|dt| Utc.from_local_datetime(&dt).single())
                    .unwrap_or(now - Duration::hours(24)),
                "last_day" => now - Duration::days(1),
                "last_week" => now - Duration::weeks(1),
                "last_month" => now - Duration::days(30),
                "last_year" => now - Duration::days(365),
                _ => return None,
            };

            Some(TimeRange { start, end: now })
        })
    }

    async fn refine_params_from_guidance(
        &self,
        current_params: &ContextSearchDerivation,
        _guidance: &str,
        validation: &KnowledgeBaseSearchValidation,
    ) -> Result<ContextSearchDerivation, AppError> {
        let mut refined = current_params.clone();

        // Extract suggested search terms from content gaps
        let mut new_keywords = Vec::new();
        for gap in &validation.content_gaps {
            new_keywords.extend(gap.suggested_search_terms.clone());
        }

        // Update boost keywords
        if !new_keywords.is_empty() {
            refined.filters.boost_keywords = Some(new_keywords);
        }

        // Adjust relevance threshold based on validation
        if validation.completeness_score < 0.5 {
            // Lower threshold to get more results
            refined.filters.min_relevance *= 0.8;
        }

        // Increase max results if we need more
        if validation.relevance_assessment.missing_information.len() > 2 {
            refined.filters.max_results = (refined.filters.max_results as f32 * 1.5) as i32;
        }

        Ok(refined)
    }

    fn summarize_results(&self, results: &[ContextSearchResult]) -> String {
        if results.is_empty() {
            return "No results found".to_string();
        }

        let doc_count = results
            .iter()
            .filter_map(|r| match &r.source {
                ContextSource::KnowledgeBase { document_id, .. } => Some(document_id),
                _ => None,
            })
            .collect::<std::collections::HashSet<_>>()
            .len();

        let titles: Vec<String> = results
            .iter()
            .filter_map(|r| r.metadata.get("title").and_then(|t| t.as_str()))
            .take(3)
            .map(|s| s.to_string())
            .collect();

        format!(
            "Found {} results from {} documents. Sample titles: {}",
            results.len(),
            doc_count,
            if titles.is_empty() {
                "N/A".to_string()
            } else {
                titles.join(", ")
            }
        )
    }

    fn sample_results(&self, results: &[ContextSearchResult], count: usize) -> Vec<String> {
        results
            .iter()
            .take(count)
            .map(|r| {
                let title = r
                    .metadata
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("Untitled");
                let preview = if r.content.len() > 200 {
                    format!("{}...", &r.content[..200])
                } else {
                    r.content.clone()
                };
                format!("[{}] {}", title, preview)
            })
            .collect()
    }

    fn extract_conversation_content(&self, content: &ConversationContent) -> String {
        serde_json::to_string(content).unwrap()
    }
}
