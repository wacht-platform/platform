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
        _query: &str,
        query_embedding: &[f32],
        kb_id: Option<i64>,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        // If kb_id is provided, search specific KB, otherwise search all
        let mut results = Vec::new();

        if let Some(kb_id) = kb_id {
            let search_results = SearchKnowledgeBaseEmbeddingsCommand::new(
                kb_id,
                query_embedding.to_vec(),
                filters.max_results as u64,
            )
            .execute(&self.app_state)
            .await?;

            for result in search_results {
                let relevance = (1.0 - (result.score / 2.0)).max(0.0);
                if relevance >= filters.min_relevance {
                    results.push(ContextSearchResult {
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
                    });
                }
            }
        }

        Ok(results)
    }

    async fn search_dynamic_context(
        &self,
        _query: &str,
        _query_embedding: &[f32],
        _context_type: Option<String>,
        _filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        // Search dynamic context using SearchAgentDynamicContextQuery
        // Implementation depends on your dynamic context query structure
        Ok(Vec::new()) // Placeholder
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
        _query: &str,
        _query_embedding: &[f32],
        _context_id: i64,
        _filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        // Search conversations using the new conversations table
        // This would need a new query implementation
        Ok(Vec::new()) // Placeholder
    }

    async fn search_all(
        &self,
        query: &str,
        query_embedding: &[f32],
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        // Search all sources and combine results (excluding conversations)
        let mut all_results = Vec::new();

        // Search memories
        let memory_results = self
            .search_memories(query, query_embedding, None, filters)
            .await?;
        all_results.extend(memory_results);

        // Search knowledge bases if available
        // Note: This would search all KBs associated with the agent
        // You might want to implement a method to get all KB IDs for the agent
        
        // Search dynamic context
        let dynamic_results = self
            .search_dynamic_context(query, query_embedding, None, filters)
            .await?;
        all_results.extend(dynamic_results);

        // Sort by relevance
        all_results.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap());

        // Limit to max_results
        all_results.truncate(filters.max_results);

        Ok(all_results)
    }
}
