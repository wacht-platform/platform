use crate::agentic::gemini_client::GeminiClient;
use crate::template::{AgentTemplates, render_template_with_prompt};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared::commands::{Command, GenerateEmbeddingCommand, SearchKnowledgeBaseEmbeddingsCommand};
use shared::error::AppError;
use shared::dto::json::{ContextFilters, ContextSearchResult, ContextSource, SearchMode, TimeRange};
use shared::models::{
    AiAgentWithFeatures, ConversationContent,
    ConversationMessageType, ConversationRecord,
};
use shared::queries::{
    FullTextSearchKnowledgeBaseQuery, GetDocumentChunksQuery, GetKnowledgeBaseDocumentsQuery,
    HybridSearchKnowledgeBaseQuery, Query, SearchMemoriesQuery,
};
use shared::state::AppState;
use std::collections::HashSet;

use super::ObjectiveDefinition;

const RELEVANCE_SCORE_DIVISOR: f32 = 2.0;

// Internal types for LLM response parsing
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContextSearchDerivation {
    search_query: String,
    search_scope: SearchScope,
    filters: LLMFilters,
    #[serde(skip_serializing_if = "Option::is_none")]
    list_documents_params: Option<ListDocumentsParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    read_document_params: Option<ReadDocumentParams>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LLMFilters {
    max_results: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    boost_keywords: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_range: Option<String>,
    search_mode: LLMSearchMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LLMSearchMode {
    Semantic,
    Keyword,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ListDocumentsParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    knowledge_base_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    keyword_filter: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReadDocumentParams {
    document_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    chunk_range: Option<ChunkRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    keywords: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChunkRange {
    start: i32,
    end: i32,
}

// Internal enums
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SearchScope {
    KnowledgeBase,
    Experience,
    Universal,
    ListKnowledgeBaseDocuments,
    ReadKnowledgeBaseDocuments,
    GatheredContext,
}

#[derive(Clone)]
pub struct ContextGatheringOrchestrator {
    app_state: AppState,
    agent: AiAgentWithFeatures,
}

impl ContextGatheringOrchestrator {
    pub fn new(app_state: AppState, agent: AiAgentWithFeatures) -> Self {
        Self { app_state, agent }
    }

    pub async fn gather_context(
        &self,
        conversations: &[ConversationRecord],
        current_objective: &Option<ObjectiveDefinition>,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        println!("\n=== Context Gathering Orchestrator ===");

        let mut all_results = Vec::new();
        let mut previous_searches = Vec::new();

        const MAX_ITERATIONS: usize = 5;

        for iteration in 1..=MAX_ITERATIONS {
            println!("\nIteration {}", iteration);

            let derivation = self
                .derive_search_strategy(conversations, current_objective, &previous_searches)
                .await?;

            println!("Search scope: {:?}", derivation.search_scope);

            if matches!(derivation.search_scope, SearchScope::GatheredContext) {
                println!("LLM decided context gathering is complete");
                break;
            }

            let results = self.execute_search(&derivation).await?;

            previous_searches.push(json!({
                "iteration": iteration,
                "search_query": derivation.search_query,
                "search_scope": format!("{:?}", derivation.search_scope),
                "results_count": results.len(),
            }));

            all_results.extend(results);

            if !all_results.is_empty() && iteration > 1 {
                println!(
                    "Found {} total results across {} iterations",
                    all_results.len(),
                    iteration
                );
            }
        }

        let total_results_count = all_results.len();
        let deduped_results = self.deduplicate_results(all_results);

        println!("\n=== Context Gathering Summary ===");
        println!("Total iterations: {}", previous_searches.len());
        println!("Total results before deduplication: {}", total_results_count);
        println!("Total results after deduplication: {}", deduped_results.len());
        
        // Count results by source type
        let mut kb_count = 0;
        let mut memory_count = 0;
        for result in &deduped_results {
            match &result.source {
                ContextSource::KnowledgeBase { .. } => kb_count += 1,
                ContextSource::Memory { .. } => memory_count += 1,
                _ => {}
            }
        }
        println!("Results by source: KB={}, Memory={}", kb_count, memory_count);
        
        Ok(deduped_results)
    }

    async fn derive_search_strategy(
        &self,
        conversations: &[ConversationRecord],
        current_objective: &Option<ObjectiveDefinition>,
        previous_searches: &[Value],
    ) -> Result<ContextSearchDerivation, AppError> {
        let request_body = render_template_with_prompt(
            AgentTemplates::CONTEXT_SEARCH_DERIVATION,
            json!({
                "conversation_history": self.format_conversation_history(conversations),
                "current_objective": current_objective,
                "available_knowledge_bases": self.agent.knowledge_bases.clone(),
                "has_previous_searches": !previous_searches.is_empty(),
                "previous_search_count": previous_searches.len(),
                "previous_search_results": previous_searches,
            }),
        )
        .map_err(|e| AppError::Internal(format!("Failed to render template: {}", e)))?;

        let derivation = self
            .create_gemini_client()?
            .generate_structured_content::<ContextSearchDerivation>(request_body)
            .await?;

        println!("\n=== LLM Search Strategy Decision ===");
        println!("Search Scope: {:?}", derivation.search_scope);
        println!("Search Query: \"{}\"", derivation.search_query);
        println!("Search Mode: {:?}", derivation.filters.search_mode);
        println!("Max Results: {}", derivation.filters.max_results);
        if let Some(boost) = &derivation.filters.boost_keywords {
            println!("Boost Keywords: {:?}", boost);
        }
        if let Some(time) = &derivation.filters.time_range {
            println!("Time Range: {}", time);
        }

        Ok(derivation)
    }

    async fn execute_search(
        &self,
        derivation: &ContextSearchDerivation,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let filters = self.convert_filters(&derivation.filters)?;
        let max_results = derivation.filters.max_results as usize;
        let query = &derivation.search_query;

        println!("\n=== Executing Search ===");
        println!("Search Scope: {:?}", derivation.search_scope);
        println!("Query: \"{}\"", query);
        println!("Max Results: {}", max_results);
        println!("Search Mode: {:?}", filters.search_mode);
        println!("Filters: {:?}", filters);

        match &derivation.search_scope {
            SearchScope::KnowledgeBase => {
                self.execute_knowledge_base_search(query, None, max_results, &filters)
                    .await
            }
            SearchScope::Experience => self.search_memories(query, max_results).await,
            SearchScope::Universal => {
                self.execute_universal_search(query, max_results, &filters)
                    .await
            }
            SearchScope::ListKnowledgeBaseDocuments => {
                self.list_knowledge_base_documents(derivation).await
            }
            SearchScope::ReadKnowledgeBaseDocuments => {
                self.read_knowledge_base_documents(derivation).await
            }
            SearchScope::GatheredContext => Ok(Vec::new()),
        }
    }

    fn convert_filters(&self, llm_filters: &LLMFilters) -> Result<ContextFilters, AppError> {
        let search_mode = match &llm_filters.search_mode {
            LLMSearchMode::Semantic => SearchMode::Vector,
            LLMSearchMode::Keyword => SearchMode::FullText,
            LLMSearchMode::Hybrid => SearchMode::Hybrid {
                vector_weight: 0.7,
                text_weight: 0.3,
            },
        };

        let time_range = llm_filters.time_range.as_ref().and_then(|tr| {
            let now = Utc::now();
            match tr.as_str() {
                "last_hour" => Some(TimeRange {
                    start: now - Duration::hours(1),
                    end: now,
                }),
                "today" => Some(TimeRange {
                    start: now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc(),
                    end: now,
                }),
                "last_week" => Some(TimeRange {
                    start: now - Duration::weeks(1),
                    end: now,
                }),
                "last_month" => Some(TimeRange {
                    start: now - Duration::days(30),
                    end: now,
                }),
                "last_year" => Some(TimeRange {
                    start: now - Duration::days(365),
                    end: now,
                }),
                _ => None,
            }
        });

        Ok(ContextFilters {
            max_results: llm_filters.max_results as usize,
            time_range,
            search_mode,
            boost_keywords: llm_filters.boost_keywords.clone(),
        })
    }

    async fn execute_knowledge_base_search(
        &self,
        query: &str,
        kb_id: Option<i64>,
        max_results: usize,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        // Determine which KB IDs to search
        let kb_ids = if let Some(kb_id) = kb_id {
            vec![kb_id]
        } else {
            self.agent.knowledge_bases.iter().map(|kb| kb.id).collect()
        };

        println!("\n--- Knowledge Base Search ---");
        println!("KB IDs to search: {:?}", kb_ids);
        println!("Query: \"{}\"", query);
        println!("Search mode: {:?}", filters.search_mode);
        println!("Max results: {}", max_results);
        if let Some(keywords) = &filters.boost_keywords {
            println!("Boost keywords: {:?}", keywords);
        }

        let results = match &filters.search_mode {
            SearchMode::FullText => {
                // Apply boost keywords to the query if provided
                let enhanced_query = if let Some(keywords) = &filters.boost_keywords {
                    let enhanced = format!("{} {}", query, keywords.join(" "));
                    println!("Full-text search with boost keywords: \"{}\"", enhanced);
                    enhanced
                } else {
                    query.to_string()
                };
                self.search_kb_fulltext(&kb_ids, &enhanced_query, max_results).await?
            }
            SearchMode::Vector => {
                let query_embedding = self.generate_embedding(query).await?;
                self.search_kb_vector(&kb_ids, &query_embedding, max_results)
                    .await?
            }
            SearchMode::Hybrid {
                vector_weight,
                text_weight,
            } => {
                let query_embedding = self.generate_embedding(query).await?;
                self.search_kb_hybrid(
                    &kb_ids,
                    query,
                    &query_embedding,
                    max_results,
                    filters.boost_keywords.clone(),
                    *vector_weight,
                    *text_weight,
                )
                .await?
            }
        };

        println!("Knowledge base search returned {} results", results.len());
        Ok(results)
    }

    async fn execute_universal_search(
        &self,
        query: &str,
        max_results: usize,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        println!("\n>>> Universal search (KB + Memories)");
        println!("    Query: \"{}\"", query);
        println!("    Max results: {}", max_results);

        let mut all_results = Vec::new();

        let kb_results = self
            .execute_knowledge_base_search(query, None, max_results, filters)
            .await?;
        println!("    KB search returned {} results", kb_results.len());
        all_results.extend(kb_results);

        let memory_results = self.search_memories(query, max_results).await?;
        println!("    Memory search returned {} results", memory_results.len());
        all_results.extend(memory_results);

        let final_results = self.sort_and_limit_results(all_results, max_results);
        println!("    Total universal search results: {}", final_results.len());
        Ok(final_results)
    }

    // Helper method to generate embeddings only when needed
    async fn generate_embedding(&self, query: &str) -> Result<Vec<f32>, AppError> {
        println!("\n    Generating embedding for query: \"{}\"", query);
        let embedding_result = GenerateEmbeddingCommand::new(query.to_string())
            .execute(&self.app_state)
            .await?;
        println!("    Successfully generated embedding of length {}", embedding_result.len());
        Ok(embedding_result)
    }

    async fn search_kb_fulltext(
        &self,
        kb_ids: &[i64],
        query: &str,
        max_results: usize,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        println!("\n>>> Full-text search");
        println!("    Query: \"{}\"", query);
        println!("    KB IDs: {:?}", kb_ids);
        println!("    Max results: {}", max_results);

        let results = FullTextSearchKnowledgeBaseQuery {
            knowledge_base_ids: kb_ids.to_vec(),
            query_text: query.to_string(),
            deployment_id: self.agent.deployment_id,
            max_results: max_results as i32,
        }
        .execute(&self.app_state)
        .await?;

        println!("    Full-text search found {} results", results.len());

        Ok(results
            .into_iter()
            .map(|r| ContextSearchResult {
                source: ContextSource::KnowledgeBase {
                    kb_id: r.knowledge_base_id,
                    document_id: r.document_id,
                },
                content: r.content,
                relevance_score: r.text_rank as f64,
                metadata: json!({
                    "document_title": r.document_title,
                    "chunk_index": r.chunk_index,
                }),
            })
            .collect())
    }

    async fn search_kb_vector(
        &self,
        kb_ids: &[i64],
        query_embedding: &[f32],
        max_results: usize,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        println!("\n>>> Vector search");
        println!("    KB IDs: {:?}", kb_ids);
        println!("    Embedding length: {}", query_embedding.len());
        println!("    Max results: {}", max_results);
        println!("    Min similarity: ~40% (max distance: 1.2)");

        let results = SearchKnowledgeBaseEmbeddingsCommand::new(
            kb_ids.to_vec(),
            query_embedding.to_vec(),
            max_results as u64,
        )
        .execute(&self.app_state)
        .await?;

        println!("    Vector search found {} results", results.len());

        Ok(results
            .into_iter()
            .map(|r| ContextSearchResult {
                source: ContextSource::KnowledgeBase {
                    kb_id: r.knowledge_base_id,
                    document_id: r.document_id,
                },
                content: r.content,
                relevance_score: r.score as f64,
                metadata: json!({
                    "chunk_index": r.chunk_index,
                    "document_title": r.document_title,
                    "document_description": r.document_description,
                }),
            })
            .collect())
    }

    async fn search_kb_hybrid(
        &self,
        kb_ids: &[i64],
        query: &str,
        query_embedding: &[f32],
        max_results: usize,
        boost_keywords: Option<Vec<String>>,
        vector_weight: f32,
        text_weight: f32,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let mut query_text = query.to_string();
        if let Some(keywords) = &boost_keywords {
            query_text = format!("{} {}", query_text, keywords.join(" "));
        }

        println!("\n>>> Hybrid search");
        println!("    Query: \"{}\"", query);
        println!("    KB IDs: {:?}", kb_ids);
        println!("    Max results: {}", max_results);
        println!("    Vector weight: {}", vector_weight);
        println!("    Text weight: {}", text_weight);
        if boost_keywords.is_some() {
            println!("    Boost keywords: {:?}", boost_keywords);
            println!("    Enhanced query: \"{}\"", query_text);
        }

        let results = HybridSearchKnowledgeBaseQuery {
            knowledge_base_ids: kb_ids.to_vec(),
            query_text,
            query_embedding: query_embedding.to_vec(),
            vector_weight: vector_weight as f64,
            text_weight: text_weight as f64,
            deployment_id: self.agent.deployment_id,
            max_results: max_results as i32,
        }
        .execute(&self.app_state)
        .await?;

        println!("    Hybrid search found {} results", results.len());

        Ok(results
            .into_iter()
            .map(|r| ContextSearchResult {
                source: ContextSource::KnowledgeBase {
                    kb_id: r.knowledge_base_id,
                    document_id: r.document_id,
                },
                content: r.content,
                relevance_score: (r.combined_score / RELEVANCE_SCORE_DIVISOR as f64),
                metadata: json!({
                    "document_title": r.document_title,
                    "chunk_index": r.chunk_index,
                    "text_rank": r.text_rank,
                    "vector_similarity": r.vector_similarity,
                }),
            })
            .collect())
    }

    async fn search_memories(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        println!("\n>>> Memory search");
        println!("    Query: \"{}\"", query);
        println!("    Max results: {}", max_results);
        println!("    Agent ID: {}", self.agent.id);

        // Generate embedding for the query
        let query_embedding = self.generate_embedding(query).await?;
        println!("    Generated embedding length: {}", query_embedding.len());

        let results = SearchMemoriesQuery {
            agent_id: self.agent.id,
            query_embedding,
            limit: max_results as i64,
            memory_type_filter: vec![],
            min_importance: None,
            time_range: None,
        }
        .execute(&self.app_state)
        .await?;

        println!("    Memory search found {} results", results.len());

        Ok(results
            .into_iter()
            .map(|m| ContextSearchResult {
                source: ContextSource::Memory {
                    memory_id: m.id,
                    category: m.memory_type.clone(),
                },
                content: m.content,
                relevance_score: m.importance,
                metadata: json!({
                    "memory_type": m.memory_type,
                    "created_at": m.created_at,
                    "similarity_score": m.score,
                }),
            })
            .collect())
    }

    async fn list_knowledge_base_documents(
        &self,
        params: &ContextSearchDerivation,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        println!("\n>>> List Knowledge Base Documents");
        
        let kb_ids = if let Some(kb_id) = params
            .list_documents_params
            .as_ref()
            .and_then(|p| p.knowledge_base_id)
        {
            vec![kb_id]
        } else {
            self.agent.knowledge_bases.iter().map(|kb| kb.id).collect()
        };

        println!("    KB IDs: {:?}", kb_ids);
        
        if let Some(keyword) = params
            .list_documents_params
            .as_ref()
            .and_then(|p| p.keyword_filter.as_ref())
        {
            println!("    Keyword filter: \"{}\"", keyword);
        }

        let mut all_documents = Vec::new();

        for kb_id in kb_ids {
            let documents = GetKnowledgeBaseDocumentsQuery::new(kb_id, 100, 0)
                .execute(&self.app_state)
                .await?;

            for doc in documents {
                // Apply keyword filter if provided
                if let Some(keyword) = params
                    .list_documents_params
                    .as_ref()
                    .and_then(|p| p.keyword_filter.as_ref())
                {
                    if !doc.title.to_lowercase().contains(&keyword.to_lowercase()) {
                        continue;
                    }
                }

                all_documents.push(ContextSearchResult {
                    source: ContextSource::KnowledgeBase {
                        kb_id,
                        document_id: doc.id,
                    },
                    content: format!("Document: {} (ID: {})", doc.title, doc.id),
                    relevance_score: 1.0,
                    metadata: json!({
                        "document_id": doc.id,
                        "document_title": doc.title,
                        "created_at": doc.created_at,
                    }),
                });
            }
        }

        println!("    Found {} documents total", all_documents.len());
        Ok(all_documents)
    }

    async fn read_knowledge_base_documents(
        &self,
        params: &ContextSearchDerivation,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        println!("\n>>> Read Knowledge Base Documents");
        
        let read_params = params
            .read_document_params
            .as_ref()
            .ok_or_else(|| AppError::BadRequest("read_document_params required".to_string()))?;

        let document_id = read_params.document_id;
        let limit = read_params.limit.unwrap_or(10) as usize;

        println!("    Document ID: {}", document_id);
        println!("    Limit: {}", limit);

        let mut query = GetDocumentChunksQuery::new(document_id);

        if let Some(keywords) = &read_params.keywords {
            println!("    Keywords: {:?}", keywords);
            query = query.with_keywords(keywords.clone());
        }

        if let Some(range) = &read_params.chunk_range {
            println!("    Chunk range: {} to {}", range.start, range.end);
            query = query.with_chunk_range(range.start, range.end);
        }

        query = query.with_limit(limit);

        let chunks = query.execute(&self.app_state).await?;
        println!("    Found {} chunks", chunks.len());

        let kb_id = chunks.first().map(|c| c.knowledge_base_id).unwrap_or(0);

        Ok(chunks
            .into_iter()
            .map(|chunk| ContextSearchResult {
                source: ContextSource::KnowledgeBase { kb_id, document_id },
                content: chunk.content,
                relevance_score: 1.0,
                metadata: json!({
                    "chunk_index": chunk.chunk_index,
                }),
            })
            .collect())
    }

    fn sort_and_limit_results(
        &self,
        mut results: Vec<ContextSearchResult>,
        limit: usize,
    ) -> Vec<ContextSearchResult> {
        results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        results
    }

    fn deduplicate_results(&self, results: Vec<ContextSearchResult>) -> Vec<ContextSearchResult> {
        let mut unique_results = Vec::new();
        let mut seen_content = HashSet::new();

        for result in results {
            let content_hash = format!("{:?}", result.content);
            if !seen_content.contains(&content_hash) {
                seen_content.insert(content_hash);
                unique_results.push(result);
            }
        }

        unique_results
    }

    fn format_conversation_history(&self, conversations: &[ConversationRecord]) -> Vec<Value> {
        conversations
            .iter()
            .map(|conv| {
                json!({
                    "role": self.map_conversation_type_to_role(&conv.message_type),
                    "content": self.extract_conversation_content(&conv.content),
                    "timestamp": conv.created_at,
                })
            })
            .collect()
    }

    fn map_conversation_type_to_role(&self, msg_type: &ConversationMessageType) -> &'static str {
        match msg_type {
            ConversationMessageType::UserMessage => "user",
            _ => "model",
        }
    }

    fn extract_conversation_content(&self, content: &ConversationContent) -> String {
        match content {
            ConversationContent::UserMessage { message } => message.clone(),
            ConversationContent::AssistantAcknowledgment {
                acknowledgment_message,
                ..
            } => acknowledgment_message.clone(),
            ConversationContent::AgentResponse { response, .. } => response.clone(),
            _ => serde_json::to_string(content).unwrap_or_default(),
        }
    }

    fn create_gemini_client(&self) -> Result<GeminiClient, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_else(|_| "test-key".to_string());
        Ok(GeminiClient::new(
            api_key,
            Some("gemini-2.5-flash".to_string()),
        ))
    }
}
