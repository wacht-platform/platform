use serde_json::json;
use shared::commands::{Command, GenerateEmbeddingCommand, SearchKnowledgeBaseEmbeddingsCommand};
use shared::error::AppError;
use shared::models::{
    AiAgentWithFeatures, ContextAction, ContextEngineParams, ContextFilters, ContextSearchResult,
    ContextSource, SearchMode,
};
use shared::queries::{
    DebugKnowledgeBaseContentQuery, DebugTextSearchQuery, FullTextSearchKnowledgeBaseQuery,
    HybridSearchKnowledgeBaseQuery, HybridSearchMemoriesQuery, Query, SearchMemoriesQuery,
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

    fn create_enhanced_kb_search_result(
        &self,
        kb_id: i64,
        document_id: i64,
        chunk_index: i32,
        content: String,
        document_title: Option<String>,
        document_description: Option<String>,
        relevance: f64,
        metadata: serde_json::Value,
    ) -> ContextSearchResult {
        let mut enhanced_metadata = metadata;
        if let Some(obj) = enhanced_metadata.as_object_mut() {
            obj.insert("chunk_index".to_string(), json!(chunk_index));
            obj.insert("document_id".to_string(), json!(document_id));
            if let Some(title) = document_title {
                obj.insert("document_title".to_string(), json!(title));
            }
            if let Some(desc) = document_description {
                obj.insert("document_description".to_string(), json!(desc));
            }
        }

        ContextSearchResult {
            source: ContextSource::KnowledgeBase { kb_id, document_id },
            content,
            relevance_score: relevance,
            metadata: enhanced_metadata,
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

    async fn search_memories_hybrid(
        &self,
        query: &str,
        query_embedding: &[f32],
        category: Option<String>,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        match &filters.search_mode {
            SearchMode::Vector => {
                // Use existing vector search
                self.search_memories(query, query_embedding, category, filters)
                    .await
            }
            SearchMode::FullText => {
                // Full-text only not implemented for memories yet
                tracing::warn!(
                    "Full-text search for memories not implemented, using vector search"
                );
                self.search_memories(query, query_embedding, category, filters)
                    .await
            }
            SearchMode::Hybrid {
                vector_weight,
                text_weight,
            } => {
                let query = HybridSearchMemoriesQuery {
                    query_text: query.to_string(),
                    query_embedding: query_embedding.to_vec(),
                    agent_id: self.agent.id,
                    context_id: self.context_id,
                    max_results: filters.max_results as i32,
                    min_relevance: filters.min_relevance,
                    vector_weight: *vector_weight as f64,
                    text_weight: *text_weight as f64,
                };

                let results = query.execute(&self.app_state).await?;

                Ok(results
                    .into_iter()
                    .filter(|result| {
                        // Filter by category if specified
                        category
                            .as_ref()
                            .map(|cat| &result.memory_type == cat)
                            .unwrap_or(true)
                    })
                    .map(|result| ContextSearchResult {
                        source: ContextSource::Memory {
                            memory_id: result.id,
                            category: result.memory_type.clone(),
                        },
                        content: result.content,
                        relevance_score: result.combined_score,
                        metadata: json!({
                            "importance": result.importance,
                            "created_at": result.created_at,
                            "vector_similarity": result.vector_similarity,
                            "text_rank": result.text_rank,
                            "combined_score": result.combined_score,
                            "search_mode": "hybrid",
                        }),
                    })
                    .collect())
            }
        }
    }

    async fn search_dynamic_context_hybrid(
        &self,
        query: &str,
        query_embedding: &[f32],
        context_type: Option<String>,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        // For now, dynamic context continues to use vector search
        // This can be extended to support hybrid search similar to KB
        self.search_dynamic_context(query, query_embedding, context_type, filters)
            .await
    }
}
