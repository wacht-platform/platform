use crate::agentic::gemini_client::GeminiClient;
use crate::template::{AgentTemplates, render_template_with_prompt};
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use shared::commands::{Command, GenerateEmbeddingCommand, SearchKnowledgeBaseEmbeddingsCommand};
use shared::dto::json::agent_executor::ObjectiveDefinition;
use shared::dto::json::context_orchestrator::{
    ContextSearchDerivation, LLMFilters, LLMSearchMode, SearchScope,
};
use shared::dto::json::{
    ContextFilters, ContextSearchResult, ContextSource, SearchMode, TimeRange,
};
use shared::error::AppError;
use shared::models::{
    AiAgentWithFeatures, ConversationContent, ConversationMessageType, ConversationRecord,
};
use shared::queries::{
    FullTextSearchKnowledgeBaseQuery, GetDocumentChunksQuery, GetKnowledgeBaseDocumentsQuery,
    HybridSearchKnowledgeBaseQuery, Query, SearchConversationsQuery,
};
use shared::state::AppState;
use std::collections::HashSet;

const RELEVANCE_SCORE_DIVISOR: f32 = 2.0;

pub struct ContextOrchestrator {
    app_state: AppState,
    agent: AiAgentWithFeatures,
    context_id: i64,
}

impl ContextOrchestrator {
    pub fn new(app_state: AppState, agent: AiAgentWithFeatures, context_id: i64) -> Self {
        Self { app_state, agent, context_id }
    }

    pub async fn gather_context(
        &self,
        conversations: &[ConversationRecord],
        current_objective: &Option<ObjectiveDefinition>,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let mut all_results = Vec::new();
        let mut previous_searches = Vec::new();

        const MAX_ITERATIONS: usize = 5;

        for iteration in 1..=MAX_ITERATIONS {
            eprintln!("\n=== Context Search Iteration {iteration} ===");
            eprintln!("Previous searches: {previous_searches:?}");

            let derivation = self
                .derive_search_strategy(conversations, current_objective, &previous_searches)
                .await?;

            eprintln!("Derivation result:");
            eprintln!("  - Search scope: {:?}", derivation.search_scope);
            eprintln!("  - Search query: {}", derivation.search_query);
            eprintln!("  - Search mode: {:?}", derivation.filters.search_mode);
            eprintln!("  - Max results: {}", derivation.filters.max_results);
            if let Some(keywords) = &derivation.filters.boost_keywords {
                eprintln!("  - Boost keywords: {keywords:?}");
            }

            if matches!(derivation.search_scope, SearchScope::GatheredContext) {
                eprintln!("Search complete - gathered_context signal received");
                break;
            }

            let results = self.execute_search(&derivation).await?;
            eprintln!("Search returned {} results", results.len());

            let search_record = json!({
                "iteration": iteration,
                "search_query": derivation.search_query,
                "search_scope": format!("{:?}", derivation.search_scope),
                "search_mode": format!("{:?}", derivation.filters.search_mode),
                "max_results": derivation.filters.max_results,
                "knowledge_base_ids": derivation.filters.knowledge_base_ids.clone(),
                "boost_keywords": derivation.filters.boost_keywords.clone(),
                "time_range": derivation.filters.time_range.clone(),
                "results_count": results.len(),
            });

            eprintln!("Adding to previous searches: {search_record:?}");
            previous_searches.push(search_record);

            // Move results into all_results instead of cloning
            all_results.extend(results);

            // Clear memory after a threshold to prevent unbounded growth
            if all_results.len() > 1000 {
                eprintln!(
                    "Warning: Context search accumulated {} results, truncating to prevent memory issues",
                    all_results.len()
                );
                all_results.truncate(500);
            }
        }

        let mut deduped_results = self.deduplicate_results(all_results);

        // Add a system message with search iteration details
        if !previous_searches.is_empty() {
            // Build search summary details
            let total_results_before_dedup = previous_searches
                .iter()
                .filter_map(|s| s.get("results_count").and_then(|v| v.as_i64()))
                .sum::<i64>();

            let search_types: Vec<String> = previous_searches
                .iter()
                .map(|s| {
                    let mode = s
                        .get("search_mode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let scope = s
                        .get("search_scope")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let count = s.get("results_count").and_then(|v| v.as_i64()).unwrap_or(0);
                    format!("{mode} search in {scope} scope found {count} chunks")
                })
                .collect();

            deduped_results.insert(0, ContextSearchResult {
                source: ContextSource::System,
                content: format!(
                    "Search summary: {} unique content chunks found from {} total chunks across {} search iterations. Search details: {}",
                    deduped_results.len(),
                    total_results_before_dedup,
                    previous_searches.len(),
                    search_types.join("; ")
                ),
                relevance_score: 1.0,
                metadata: json!({
                    "is_search_summary": true,
                    "total_iterations": previous_searches.len(),
                    "search_iterations": previous_searches,
                    "total_unique_results": deduped_results.len(),
                    "message_type": "search_summary",
                    "note": "This is a summary of the search process, not a document",
                }),
            });
        }

        Ok(deduped_results)
    }

    async fn derive_search_strategy(
        &self,
        conversations: &[ConversationRecord],
        current_objective: &Option<ObjectiveDefinition>,
        previous_searches: &[Value],
    ) -> Result<ContextSearchDerivation, AppError> {
        eprintln!("\n=== Context Search Derivation Request ===");

        // Log the last user message
        if let Some(last_user_msg) = conversations
            .iter()
            .rev()
            .find(|c| matches!(c.message_type, ConversationMessageType::UserMessage))
        {
            if let ConversationContent::UserMessage { message } = &last_user_msg.content {
                eprintln!("Last user message: \"{message}\"");
            }
        }

        eprintln!("Current objective: {current_objective:?}");
        eprintln!("Has previous searches: {}", !previous_searches.is_empty());
        eprintln!("Previous search count: {}", previous_searches.len());
        if !previous_searches.is_empty() {
            eprintln!("Previous searches summary:");
            for (i, search) in previous_searches.iter().enumerate() {
                eprintln!(
                    "  Search {}: query='{}', scope={}, results={}",
                    i + 1,
                    search
                        .get("search_query")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown"),
                    search
                        .get("search_scope")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown"),
                    search
                        .get("results_count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0)
                );
            }
        }

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
        .map_err(|e| AppError::Internal(format!("Failed to render template: {e}")))?;

        let derivation = self
            .create_gemini_client()?
            .generate_structured_content::<ContextSearchDerivation>(request_body)
            .await?;

        eprintln!("\n=== Gemini Derivation Decision ===");
        eprintln!("Decided to search for: \"{}\"", derivation.search_query);
        eprintln!("Search scope: {:?}", derivation.search_scope);
        eprintln!(
            "Query length: {} characters, {} words",
            derivation.search_query.len(),
            derivation.search_query.split_whitespace().count()
        );

        Ok(derivation)
    }

    async fn execute_search(
        &self,
        derivation: &ContextSearchDerivation,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let filters = self.convert_filters(&derivation.filters)?;
        let max_results = derivation.filters.max_results as usize;
        let query = &derivation.search_query;

        match &derivation.search_scope {
            SearchScope::KnowledgeBase => {
                let kb_ids = derivation.filters.knowledge_base_ids.as_ref().map(|ids| {
                    ids.iter()
                        .filter_map(|id| {
                            id.parse::<i64>()
                                .map_err(|_| {
                                    eprintln!(
                                        "Warning: Failed to parse knowledge_base_id '{id}' as i64"
                                    );
                                })
                                .ok()
                        })
                        .collect::<Vec<i64>>()
                });

                self.execute_knowledge_base_search(query, kb_ids, max_results, &filters)
                    .await
            }
            SearchScope::Experience => self.search_memories(query, max_results, &filters).await,
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
            SearchScope::Conversations => {
                self.search_conversations(query, max_results).await
            }
            SearchScope::GatheredContext => Ok(Vec::new()),
        }
    }

    fn convert_filters(&self, llm_filters: &LLMFilters) -> Result<ContextFilters, AppError> {
        let search_mode = match &llm_filters.search_mode {
            LLMSearchMode::Semantic => SearchMode::Vector,
            LLMSearchMode::Keyword => SearchMode::FullText,
            LLMSearchMode::Hybrid => {
                let vector_weight = 0.7;
                let text_weight = 0.3;

                // Validate weights sum to 1.0 (with small epsilon for floating point)
                let weight_sum = vector_weight + text_weight;
                if (weight_sum - 1.0_f32).abs() > 0.001 {
                    eprintln!(
                        "Warning: Hybrid search weights don't sum to 1.0: {vector_weight} + {text_weight} = {weight_sum}"
                    );
                }

                SearchMode::Hybrid {
                    vector_weight,
                    text_weight,
                }
            }
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
        kb_ids: Option<Vec<i64>>,
        max_results: usize,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let kb_ids =
            kb_ids.unwrap_or_else(|| self.agent.knowledge_bases.iter().map(|kb| kb.id).collect());

        let results = match &filters.search_mode {
            SearchMode::FullText => {
                let enhanced_query = if let Some(keywords) = &filters.boost_keywords {
                    let enhanced = format!("{} {}", query, keywords.join(" "));
                    enhanced
                } else {
                    query.to_string()
                };
                self.search_kb_fulltext(&kb_ids, &enhanced_query, max_results)
                    .await?
            }
            SearchMode::Vector => {
                let query_embedding = self.generate_embedding(query).await?;
                self.search_kb_vector(&kb_ids, query, &query_embedding, max_results)
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

        Ok(results)
    }

    async fn execute_universal_search(
        &self,
        query: &str,
        max_results: usize,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let mut all_results = Vec::new();

        let kb_results = self
            .execute_knowledge_base_search(query, None, max_results, filters)
            .await?;
        all_results.extend(kb_results);

        let memory_results = self.search_memories(query, max_results, filters).await?;
        all_results.extend(memory_results);

        let final_results = self.sort_and_limit_results(all_results, max_results);
        Ok(final_results)
    }

    async fn generate_embedding(&self, query: &str) -> Result<Vec<f32>, AppError> {
        let embedding_result = GenerateEmbeddingCommand::new(query.to_string())
            .with_task_type("RETRIEVAL_QUERY".to_string())
            .execute(&self.app_state)
            .await?;
        Ok(embedding_result)
    }

    async fn search_kb_fulltext(
        &self,
        kb_ids: &[i64],
        query: &str,
        max_results: usize,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let results = FullTextSearchKnowledgeBaseQuery {
            knowledge_base_ids: kb_ids.to_vec(),
            query_text: query.to_string(),
            deployment_id: self.agent.deployment_id,
            max_results: max_results as i32,
        }
        .execute(&self.app_state)
        .await?;

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
                    "query": query,
                    "search_mode": "fulltext",
                }),
            })
            .collect())
    }

    async fn search_kb_vector(
        &self,
        kb_ids: &[i64],
        query: &str,
        query_embedding: &[f32],
        max_results: usize,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let results = SearchKnowledgeBaseEmbeddingsCommand::new(
            kb_ids.to_vec(),
            query_embedding.to_vec(),
            max_results as u64,
        )
        .execute(&self.app_state)
        .await?;

        Ok(results
            .into_iter()
            .map(|r| ContextSearchResult {
                source: ContextSource::KnowledgeBase {
                    kb_id: r.knowledge_base_id,
                    document_id: r.document_id,
                },
                content: r.content,
                relevance_score: r.score,
                metadata: json!({
                    "chunk_index": r.chunk_index,
                    "document_title": r.document_title,
                    "document_description": r.document_description,
                    "query": query,
                    "search_mode": "vector",
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
                    "query": query,
                    "search_mode": "hybrid",
                    "vector_weight": vector_weight,
                    "text_weight": text_weight,
                }),
            })
            .collect())
    }

    async fn search_memories(
        &self,
        query: &str,
        max_results: usize,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let query_embedding = self.generate_embedding(query).await?;

        let time_range = filters.time_range.as_ref().map(|tr| (tr.start, tr.end));

        let results = shared::queries::SearchMemoriesWithDecayQuery {
            query_embedding,
            limit: max_results as i64,
            time_range,
        }
        .execute(&self.app_state)
        .await?;

        Ok(results
            .into_iter()
            .map(|m| ContextSearchResult {
                source: ContextSource::Memory {
                    memory_id: m.memory.id,
                    category: m.memory.memory_category.clone(),
                },
                content: m.memory.content.clone(),
                relevance_score: m.decay_adjusted_score,
                metadata: json!({
                    "memory_category": m.memory.memory_category,
                    "created_at": m.memory.created_at,
                    "similarity_score": m.similarity_score,
                    "decay_adjusted_score": m.decay_adjusted_score,
                    "query": query,
                    "search_mode": "memory_vector",
                }),
            })
            .collect())
    }

    async fn list_knowledge_base_documents(
        &self,
        params: &ContextSearchDerivation,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let list_params = params.list_documents_params.as_ref().ok_or_else(|| {
            AppError::BadRequest(
                "list_documents_params is required for ListKnowledgeBaseDocuments scope"
                    .to_string(),
            )
        })?;

        // Parse KB IDs from the request
        let kb_ids: Vec<i64> = if let Some(ids) = &list_params.knowledge_base_ids {
            ids.iter()
                .filter_map(|id| {
                    id.parse::<i64>()
                        .map_err(|_| {
                            eprintln!("Warning: Failed to parse knowledge_base_id '{id}' as i64");
                        })
                        .ok()
                })
                .collect()
        } else {
            // If no specific KBs provided, use all available KBs
            self.agent.knowledge_bases.iter().map(|kb| kb.id).collect()
        };

        if kb_ids.is_empty() {
            return Err(AppError::BadRequest(
                "No valid knowledge bases available or specified".to_string(),
            ));
        }

        let mut all_documents = Vec::new();
        let mut kb_has_next_page = std::collections::HashMap::new();

        // Calculate offset from page and limit with validation
        let page = list_params.page.max(1); // Ensure page is at least 1
        let limit = list_params.limit.max(1).min(200); // Ensure limit is between 1 and 200

        // Warn if unusual values are provided
        if list_params.page < 1 {
            eprintln!(
                "Warning: Page number {} is less than 1, using page 1",
                list_params.page
            );
        }
        if list_params.limit < 1 || list_params.limit > 200 {
            eprintln!(
                "Warning: Limit {} is outside range [1, 200], using {}",
                list_params.limit, limit
            );
        }

        // For multiple KBs, we need to distribute the limit across them
        let per_kb_limit = (limit as f32 / kb_ids.len() as f32).ceil() as i32;
        let offset = ((page - 1) * per_kb_limit) as usize;

        // Fetch from each KB
        for kb_id in &kb_ids {
            // Fetch limit + 1 to check if there's a next page for this KB
            let fetch_limit = (per_kb_limit + 1) as usize;
            let documents = GetKnowledgeBaseDocumentsQuery::new(*kb_id, fetch_limit, offset)
                .execute(&self.app_state)
                .await
                .map_err(|e| {
                    AppError::Internal(format!(
                        "Failed to fetch documents from KB {kb_id}: {e}"
                    ))
                })?;

            // Check if this KB has more pages
            let has_next_page = documents.len() > per_kb_limit as usize;
            kb_has_next_page.insert(*kb_id, has_next_page);

            // Only process up to 'per_kb_limit' documents
            let docs_to_process = if has_next_page {
                &documents[..per_kb_limit as usize]
            } else {
                &documents[..]
            };

            for doc in docs_to_process {
                // Apply keyword filter if provided
                if let Some(keyword) = &list_params.keyword_filter {
                    if !doc.title.to_lowercase().contains(&keyword.to_lowercase()) {
                        continue;
                    }
                }

                all_documents.push(ContextSearchResult {
                    source: ContextSource::KnowledgeBase {
                        kb_id: *kb_id,
                        document_id: doc.id,
                    },
                    content: format!("Document: {} (ID: {}) from KB {}", doc.title, doc.id, kb_id),
                    relevance_score: 1.0,
                    metadata: json!({
                        "document_id": doc.id,
                        "document_title": doc.title,
                        "knowledge_base_id": kb_id,
                        "created_at": doc.created_at,
                        "page": page,
                    }),
                });
            }
        }

        // Sort all documents by created_at to maintain consistent ordering
        all_documents.sort_by(|a, b| {
            let a_created = a
                .metadata
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let b_created = b
                .metadata
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            b_created.cmp(a_created) // Most recent first
        });

        // Limit total results to requested limit
        all_documents.truncate(limit as usize);

        // Check if any KB has more pages
        let any_has_next_page = kb_has_next_page.values().any(|&has_next| has_next);

        // Add a summary result indicating pagination info
        if !all_documents.is_empty() || page == 1 {
            let total_returned = all_documents.len();
            let kb_summary: Vec<String> = kb_ids
                .iter()
                .map(|kb_id| {
                    let kb_name = self
                        .agent
                        .knowledge_bases
                        .iter()
                        .find(|kb| kb.id == *kb_id)
                        .map(|kb| kb.name.as_str())
                        .unwrap_or("Unknown");
                    format!("{kb_name}({kb_id})")
                })
                .collect();

            all_documents.insert(
                0,
                ContextSearchResult {
                    source: ContextSource::System,
                    content: format!(
                        "Page {} of documents from {} knowledge bases: {}. Found {} documents. {}",
                        page,
                        kb_ids.len(),
                        kb_summary.join(", "),
                        total_returned,
                        if any_has_next_page {
                            "More documents available on the next page."
                        } else {
                            "No more pages available."
                        }
                    ),
                    relevance_score: 1.0,
                    metadata: json!({
                        "is_pagination_info": true,
                        "knowledge_base_ids": kb_ids,
                        "page": page,
                        "limit": limit,
                        "per_kb_limit": per_kb_limit,
                        "documents_returned": total_returned,
                        "has_next_page": any_has_next_page,
                        "kb_next_pages": kb_has_next_page,
                    }),
                },
            );
        }

        Ok(all_documents)
    }

    async fn read_knowledge_base_documents(
        &self,
        params: &ContextSearchDerivation,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let read_params = params.read_document_params.as_ref().ok_or_else(|| {
            AppError::BadRequest(
                "read_document_params required for ReadKnowledgeBaseDocuments scope".to_string(),
            )
        })?;

        let document_id = read_params.document_id.parse::<i64>().map_err(|_| {
            AppError::BadRequest(format!(
                "Invalid document_id format: '{}'",
                read_params.document_id
            ))
        })?;
        let limit = read_params.limit.unwrap_or(10) as usize;

        let mut query = GetDocumentChunksQuery::new(document_id);

        if let Some(keywords) = &read_params.keywords {
            query = query.with_keywords(keywords.clone());
        }

        if let Some(range) = &read_params.chunk_range {
            query = query.with_chunk_range(range.start, range.end);
        }

        query = query.with_limit(limit);

        let chunks = query.execute(&self.app_state).await.map_err(|e| {
            AppError::Internal(format!(
                "Failed to fetch document chunks for document_id {document_id}: {e}"
            ))
        })?;

        if chunks.is_empty() {
            return Err(AppError::NotFound(format!(
                "No chunks found for document_id {document_id}"
            )));
        }

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
        let mut seen_items = HashSet::new();

        for result in results {
            // Create a unique key based on source and content
            let unique_key = match &result.source {
                ContextSource::KnowledgeBase { kb_id, document_id } => {
                    // For KB results, use document_id and chunk_index if available
                    let chunk_index = result
                        .metadata
                        .get("chunk_index")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    format!("kb_{kb_id}_doc_{document_id}_chunk_{chunk_index}")
                }
                ContextSource::Memory { memory_id, .. } => {
                    format!("memory_{memory_id}")
                }
                ContextSource::Conversation { conversation_id } => {
                    format!("conversation_{conversation_id}")
                }
                ContextSource::System => {
                    // System messages should not be deduplicated
                    format!("system_{}", unique_results.len())
                }
            };

            if !seen_items.contains(&unique_key) {
                seen_items.insert(unique_key);
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
    
    async fn search_conversations(
        &self,
        _query: &str, // Query not used for conversation search currently
        max_results: usize,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let conversations = SearchConversationsQuery {
            context_id: self.context_id,
            limit: max_results as i64,
        }
        .execute(&self.app_state)
        .await?;
        
        Ok(conversations
            .into_iter()
            .enumerate()
            .map(|(idx, conv)| {
                let content = match &conv.content {
                    ConversationContent::UserMessage { message } => message.clone(),
                    ConversationContent::AgentResponse { response, .. } => response.clone(),
                    ConversationContent::AssistantAcknowledgment { acknowledgment_message, .. } => {
                        format!("Acknowledgment: {acknowledgment_message}")
                    }
                    ConversationContent::AssistantTaskExecution { task_execution, .. } => {
                        format!("Task execution: {task_execution}")
                    }
                    ConversationContent::ContextResults { query, result_count, .. } => {
                        format!("Context search for '{query}' found {result_count} results")
                    }
                    _ => format!("{:?}", conv.message_type),
                };
                
                ContextSearchResult {
                    source: ContextSource::Conversation {
                        conversation_id: conv.id,
                    },
                    content,
                    relevance_score: 1.0 - (idx as f64 * 0.01), // Newer conversations have higher relevance
                    metadata: json!({
                        "message_type": format!("{:?}", conv.message_type),
                        "timestamp": conv.timestamp,
                        "created_at": conv.created_at,
                    }),
                }
            })
            .collect())
    }
}
