use serde_json::json;
use shared::commands::{Command, GenerateEmbeddingCommand, SearchKnowledgeBaseEmbeddingsCommand};
use shared::error::AppError;
use shared::models::{
    ContextAction, ContextEngineParams, ContextFilters, ContextSearchResult, ContextSource,
};
use shared::queries::{Query, SearchMemoriesQuery};
use shared::state::AppState;

pub struct ContextEngineExecutor {
    app_state: AppState,
    context_id: i64,
    agent_id: i64,
}

impl ContextEngineExecutor {
    pub fn new(app_state: AppState, context_id: i64, agent_id: i64) -> Self {
        Self {
            app_state,
            context_id,
            agent_id,
        }
    }

    pub async fn execute(
        &self,
        params: ContextEngineParams,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        // Generate embedding for the query
        let query_embedding = GenerateEmbeddingCommand::new(params.query.clone())
            .execute(&self.app_state)
            .await?;

        let filters = params.filters.unwrap_or_default();

        match params.action {
            ContextAction::SearchKnowledgeBase { kb_id } => {
                self.search_knowledge_base(&params.query, &query_embedding, kb_id, &filters)
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

    async fn search_knowledge_base(
        &self,
        query: &str,
        query_embedding: &[f32],
        kb_id: Option<i64>,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        tracing::info!(
            "🔍 Knowledge base search - Query: '{}', KB ID: {:?}, Max results: {}, Min relevance: {}",
            query, kb_id, filters.max_results, filters.min_relevance
        );

        // If kb_id is provided, search specific KB, otherwise search all
        let mut results = Vec::new();

        if let Some(kb_id) = kb_id {
            tracing::info!("📚 Searching knowledge base ID: {}", kb_id);
            
            match SearchKnowledgeBaseEmbeddingsCommand::new(
                kb_id,
                query_embedding.to_vec(),
                filters.max_results as u64,
            )
            .execute(&self.app_state)
            .await {
                Ok(search_results) => {
                    tracing::info!("✅ Knowledge base search returned {} raw results", search_results.len());
                    
                    for (idx, result) in search_results.iter().enumerate() {
                        let relevance = (1.0 - (result.score / 2.0)).max(0.0);
                        tracing::debug!(
                            "📄 Result {}: Score: {:.4}, Relevance: {:.4}, Content length: {}, Document ID: {}",
                            idx + 1, result.score, relevance, result.content.len(), result.document_id
                        );
                        
                        if relevance >= filters.min_relevance {
                            results.push(ContextSearchResult {
                                source: ContextSource::KnowledgeBase {
                                    kb_id,
                                    document_id: result.document_id,
                                },
                                content: result.content.clone(),
                                relevance_score: relevance,
                                metadata: json!({
                                    "chunk_index": result.chunk_index,
                                    "document_id": result.document_id,
                                }),
                            });
                            tracing::info!("✅ Result {} passed relevance filter", idx + 1);
                        } else {
                            tracing::debug!("❌ Result {} filtered out (relevance {:.4} < {:.4})", idx + 1, relevance, filters.min_relevance);
                        }
                    }
                    
                    tracing::info!("📊 Knowledge base search final results: {} passed filters", results.len());
                },
                Err(e) => {
                    tracing::error!("❌ Knowledge base search failed for KB {}: {}", kb_id, e);
                    return Err(e);
                }
            }
        } else {
            tracing::warn!("⚠️ No knowledge base ID provided, returning empty results");
        }

        Ok(results)
    }

    async fn search_dynamic_context(
        &self,
        _query: &str,
        query_embedding: &[f32],
        _context_type: Option<String>,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        // Search dynamic context using SearchAgentDynamicContextQuery
        let search_results = shared::queries::agent_dynamic_context::SearchAgentDynamicContextQuery {
            execution_context_id: self.context_id,
            query_embedding: query_embedding.to_vec(),
            limit: filters.max_results as i64,
        }
        .execute(&self.app_state)
        .await?;

        let mut results = Vec::new();
        for record in search_results {
            // Convert distance to relevance score (smaller distance = higher relevance)
            let relevance = (1.0 - (record.score / 2.0)).max(0.0);
            if relevance >= filters.min_relevance {
                let source_type = record.source.clone().unwrap_or_else(|| "unknown".to_string());
                results.push(ContextSearchResult {
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
                });
            }
        }

        Ok(results)
    }

    async fn search_memories(
        &self,
        _query: &str,
        query_embedding: &[f32],
        category: Option<String>,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let memory_type_filter = if let Some(cat) = category {
            vec![cat]
        } else {
            vec![
                "procedural".to_string(),
                "semantic".to_string(),
                "episodic".to_string(),
            ]
        };

        let search_results = SearchMemoriesQuery {
            agent_id: self.agent_id,
            query_embedding: query_embedding.to_vec(),
            limit: filters.max_results as i64,
            memory_type_filter,
            min_importance: Some(filters.min_relevance),
            time_range: filters.time_range.as_ref().map(|tr| (tr.start, tr.end)),
        }
        .execute(&self.app_state)
        .await?;

        let mut results = Vec::new();
        for record in search_results {
            let relevance = (1.0 - (record.score / 2.0)).max(0.0);
            if relevance >= filters.min_relevance {
                results.push(ContextSearchResult {
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
                });
            }
        }

        Ok(results)
    }

    async fn search_conversations(
        &self,
        query: &str,
        _query_embedding: &[f32],
        context_id: i64,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        // For now, skip conversation search to avoid sqlx issues
        // TODO: Implement proper conversation search query struct
        let _rows: Vec<String> = Vec::new(); // Placeholder to avoid type annotation error

        // Return empty results for now
        Ok(Vec::new())
    }

    async fn search_all(
        &self,
        query: &str,
        query_embedding: &[f32],
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        // Search all sources and combine results (excluding conversations)
        let mut all_results = Vec::new();

        // Search memories with error handling
        match self.search_memories(query, query_embedding, None, filters).await {
            Ok(memory_results) => all_results.extend(memory_results),
            Err(e) => {
                tracing::warn!("Failed to search memories: {}", e);
                // Continue with other sources even if memory search fails
            }
        }

        // Search knowledge bases associated with this agent
        tracing::info!("🔍 Getting knowledge bases for agent {}", self.agent_id);
        match self.get_agent_knowledge_bases().await {
            Ok(knowledge_bases) => {
                tracing::info!("📚 Found {} knowledge bases for agent", knowledge_bases.len());
                if knowledge_bases.is_empty() {
                    tracing::warn!("⚠️ No knowledge bases found for agent {}", self.agent_id);
                }
                
                for kb_id in knowledge_bases {
                    tracing::info!("🔍 Searching knowledge base {}", kb_id);
                    match self.search_knowledge_base(query, query_embedding, Some(kb_id), filters).await {
                        Ok(kb_results) => {
                            tracing::info!("✅ Knowledge base {} returned {} results", kb_id, kb_results.len());
                            all_results.extend(kb_results);
                        },
                        Err(e) => {
                            tracing::error!("❌ Failed to search knowledge base {}: {}", kb_id, e);
                            // Continue with other KBs even if one fails
                        }
                    }
                }
            },
            Err(e) => {
                tracing::error!("❌ Failed to get agent knowledge bases: {}", e);
                // Continue without KB search
            }
        }
        
        // Search dynamic context with error handling
        match self.search_dynamic_context(query, query_embedding, None, filters).await {
            Ok(dynamic_results) => all_results.extend(dynamic_results),
            Err(e) => {
                tracing::warn!("Failed to search dynamic context: {}", e);
                // Continue without dynamic context search
            }
        }

        // Search conversations
        match self.search_conversations(query, query_embedding, self.context_id, filters).await {
            Ok(conversation_results) => all_results.extend(conversation_results),
            Err(e) => {
                tracing::warn!("Failed to search conversations: {}", e);
                // Continue without conversation search
            }
        }

        // Deduplicate results based on content similarity (basic deduplication)
        all_results = self.deduplicate_results(all_results);

        // Sort by relevance
        all_results.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap());

        // Limit to max_results
        all_results.truncate(filters.max_results);

        Ok(all_results)
    }

    async fn get_agent_knowledge_bases(&self) -> Result<Vec<i64>, AppError> {
        tracing::warn!("🚧 get_agent_knowledge_bases() is not implemented yet - returning empty list");
        tracing::debug!("📝 TODO: Implement proper query struct for agent knowledge bases");
        tracing::info!("🔧 Agent ID: {}, Context ID: {}", self.agent_id, self.context_id);
        
        // For now, return empty list to avoid sqlx issues
        // TODO: Implement proper query struct for agent knowledge bases
        Ok(Vec::new())
    }

    fn deduplicate_results(&self, mut results: Vec<ContextSearchResult>) -> Vec<ContextSearchResult> {
        // Simple deduplication based on content similarity
        let mut deduplicated = Vec::new();
        
        for result in results.drain(..) {
            let is_duplicate = deduplicated.iter().any(|existing: &ContextSearchResult| {
                // Consider results duplicate if content is very similar (simple approach)
                let content_similarity = self.calculate_simple_similarity(&result.content, &existing.content);
                content_similarity > 0.9 // 90% similarity threshold
            });
            
            if !is_duplicate {
                deduplicated.push(result);
            } else {
                // Keep the result with higher relevance score
                if let Some(existing_pos) = deduplicated.iter().position(|existing| {
                    self.calculate_simple_similarity(&result.content, &existing.content) > 0.9
                }) {
                    if result.relevance_score > deduplicated[existing_pos].relevance_score {
                        deduplicated[existing_pos] = result;
                    }
                }
            }
        }
        
        deduplicated
    }

    fn calculate_simple_similarity(&self, text1: &str, text2: &str) -> f32 {
        // Simple similarity calculation based on common words
        let words1: std::collections::HashSet<&str> = text1.split_whitespace().collect();
        let words2: std::collections::HashSet<&str> = text2.split_whitespace().collect();
        
        if words1.is_empty() && words2.is_empty() {
            return 1.0;
        }
        
        let intersection = words1.intersection(&words2).count();
        let union = words1.union(&words2).count();
        
        intersection as f32 / union as f32
    }
}
