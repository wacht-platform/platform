use super::gemini::GeminiClient;
use crate::template::{render_template_with_prompt, AgentTemplates};
use chrono::{Duration, Utc};
use commands::{Command, GenerateEmbeddingCommand, SearchKnowledgeBaseEmbeddingsCommand};
use common::error::AppError;
use common::state::AppState;
use dto::json::agent_executor::ObjectiveDefinition;
use dto::json::context_orchestrator::{
    ContextSearchDerivation, LLMFilters, LLMSearchMode, SearchScope,
};
use dto::json::{ContextFilters, ContextSearchResult, ContextSource, SearchMode, TimeRange};
use models::{
    AiAgentWithFeatures, ConversationContent, ConversationMessageType, ConversationRecord,
};
use queries::{
    FullTextSearchKnowledgeBaseQuery, GetDocumentChunksQuery, GetKnowledgeBaseDocumentsQuery,
    HybridSearchKnowledgeBaseQuery, Query, SearchConversationsQuery,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const RELEVANCE_SCORE_DIVISOR: f32 = 2.0;

#[derive(Debug, Clone)]
struct SearchMetrics {
    unique_sources: HashSet<String>,
    query_similarity_scores: HashMap<String, f32>,
    result_overlap_tracking: Vec<HashSet<String>>,
    consecutive_low_yields: usize,
    consecutive_duplicates: usize,
    consecutive_zero_progress: usize,
    discovery_rate_trend: Vec<usize>,
    total_unique_results: usize,
}

impl SearchMetrics {
    fn new() -> Self {
        Self {
            unique_sources: HashSet::new(),
            query_similarity_scores: HashMap::new(),
            result_overlap_tracking: Vec::new(),
            consecutive_low_yields: 0,
            consecutive_duplicates: 0,
            consecutive_zero_progress: 0,
            discovery_rate_trend: Vec::new(),
            total_unique_results: 0,
        }
    }

    fn calculate_query_similarity(&self, new_query: &str) -> f32 {
        if self.query_similarity_scores.is_empty() {
            return 0.0;
        }

        let new_words: HashSet<&str> = new_query.split_whitespace().collect();
        let mut max_similarity = 0.0;

        for existing_query in self.query_similarity_scores.keys() {
            let existing_words: HashSet<&str> = existing_query.split_whitespace().collect();
            let intersection = new_words.intersection(&existing_words).count();
            let union = new_words.union(&existing_words).count();

            if union > 0 {
                let similarity = intersection as f32 / union as f32;
                max_similarity = f32::max(max_similarity, similarity);
            }
        }

        max_similarity
    }

    fn update(&mut self, query: &str, results: &[ContextSearchResult]) -> SearchProgressData {
        // Calculate new unique sources this iteration
        let current_sources: HashSet<String> =
            results.iter().map(|r| self.get_source_key(r)).collect();

        let new_sources_count = current_sources.difference(&self.unique_sources).count();
        self.unique_sources.extend(current_sources.clone());

        // Track result overlap
        let mut overlap_percentage = 0.0;
        if !self.result_overlap_tracking.is_empty() {
            let last_results = self.result_overlap_tracking.last().unwrap();
            let intersection = current_sources.intersection(last_results).count();
            if !current_sources.is_empty() {
                overlap_percentage = (intersection as f32 / current_sources.len() as f32) * 100.0;
            }
        }
        self.result_overlap_tracking.push(current_sources);

        // Update consecutive patterns
        if results.len() <= 2 {
            self.consecutive_low_yields += 1;
        } else {
            self.consecutive_low_yields = 0;
        }

        if new_sources_count == 0 && !results.is_empty() {
            self.consecutive_duplicates += 1;
        } else {
            self.consecutive_duplicates = 0;
        }

        // Track consecutive zero progress
        if new_sources_count == 0 {
            self.consecutive_zero_progress += 1;
        } else {
            self.consecutive_zero_progress = 0;
        }

        // Track discovery rate
        self.discovery_rate_trend.push(new_sources_count);
        self.total_unique_results = self.unique_sources.len();

        // Calculate query similarity
        let query_similarity = self.calculate_query_similarity(query);
        self.query_similarity_scores
            .insert(query.to_string(), query_similarity);

        SearchProgressData {
            new_sources_this_iteration: new_sources_count,
            total_unique_sources: self.unique_sources.len(),
            result_overlap_percentage: overlap_percentage,
            query_similarity_score: query_similarity,
            consecutive_low_yields: self.consecutive_low_yields,
            consecutive_duplicates: self.consecutive_duplicates,
            discovery_rate_trend: self.get_discovery_trend(),
            information_density_declining: self.is_density_declining(),
        }
    }

    fn get_source_key(&self, result: &ContextSearchResult) -> String {
        match &result.source {
            ContextSource::KnowledgeBase { kb_id, document_id } => {
                let chunk_index = result
                    .metadata
                    .get("chunk_index")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                format!("kb_{}_{}_chunk_{}", kb_id, document_id, chunk_index)
            }
            ContextSource::Conversation { conversation_id } => format!("conv_{}", conversation_id),
            ContextSource::System => format!("system_{}", result.content.len()),
        }
    }

    fn get_discovery_trend(&self) -> String {
        if self.discovery_rate_trend.len() < 2 {
            return "insufficient_data".to_string();
        }

        let recent_trend = self
            .discovery_rate_trend
            .iter()
            .rev()
            .take(3)
            .collect::<Vec<_>>();
        let is_declining = recent_trend.windows(2).all(|w| w[0] >= w[1]);
        let is_increasing = recent_trend.windows(2).all(|w| w[0] <= w[1]);

        if is_declining {
            "declining".to_string()
        } else if is_increasing {
            "increasing".to_string()
        } else {
            "stable".to_string()
        }
    }

    fn is_density_declining(&self) -> bool {
        if self.discovery_rate_trend.len() < 3 {
            return false;
        }

        let last_three: Vec<_> = self
            .discovery_rate_trend
            .iter()
            .rev()
            .take(3)
            .cloned()
            .collect();
        last_three[0] <= last_three[1] && last_three[1] <= last_three[2]
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct SearchProgressData {
    new_sources_this_iteration: usize,
    total_unique_sources: usize,
    result_overlap_percentage: f32,
    query_similarity_score: f32,
    consecutive_low_yields: usize,
    consecutive_duplicates: usize,
    discovery_rate_trend: String,
    information_density_declining: bool,
}

pub struct ContextOrchestrator {
    app_state: AppState,
    agent: AiAgentWithFeatures,
    context_id: i64,
}

impl ContextOrchestrator {
    pub fn new(app_state: AppState, agent: AiAgentWithFeatures, context_id: i64) -> Self {
        Self {
            app_state,
            agent,
            context_id,
        }
    }

    fn get_pattern_guidance(&self, pattern: dto::json::agent_executor::SearchPattern) -> String {
        use dto::json::agent_executor::SearchPattern;
        match pattern {
            SearchPattern::Troubleshooting => {
                "Problem-solving mode: Start with symptoms and error messages. Search progression: specific errors → recent failures → configuration changes → known fixes. Goal is finding root cause and solution path."
            }
            SearchPattern::Implementation => {
                "Building mode: Start with requirements and examples. Search progression: official documentation → code samples → API references → integration patterns. Goal is understanding how to build correctly."
            }
            SearchPattern::Analysis => {
                "Investigation mode: Start with system overview. Search progression: architecture → components → interactions → performance metrics. Goal is comprehensive understanding of how things work."
            }
            SearchPattern::Historical => {
                "Timeline reconstruction: Start with recent events. Search progression: latest changes → change history → patterns over time → impact analysis. Goal is understanding evolution and causality."
            }
            SearchPattern::Verification => {
                "Fact-checking mode: Start with specific claims. Search progression: current state → expected state → validation methods → test results. Goal is confirming accuracy with evidence."
            }
            SearchPattern::Exploration => {
                "Discovery mode: Start with broad inventory. Search progression: available resources → categories → popular items → unique capabilities. Goal is mapping the landscape of possibilities."
            }
        }.to_string()
    }

    fn calculate_max_iterations(
        &self,
        pattern: dto::json::agent_executor::SearchPattern,
        expected_depth: Option<dto::json::agent_executor::SearchDepth>,
    ) -> usize {
        use dto::json::agent_executor::{SearchDepth, SearchPattern};

        // Base iterations by pattern
        let base_iterations = match pattern {
            SearchPattern::Verification => 3, // Quick checks, don't need many
            SearchPattern::Exploration => 4,  // Broad but shallow
            SearchPattern::Historical => 5,   // Moderate timeline search
            SearchPattern::Troubleshooting => 6, // Need to find root cause
            SearchPattern::Implementation => 7, // Multiple resource types
            SearchPattern::Analysis => 8,     // Comprehensive understanding
        };

        // Adjust by depth preference
        let depth_multiplier = match expected_depth {
            Some(SearchDepth::Shallow) => 0.5,
            Some(SearchDepth::Moderate) => 1.0,
            Some(SearchDepth::Deep) => 1.5,
            None => 1.0, // Default to moderate
        };

        // Calculate with bounds
        let calculated = (base_iterations as f32 * depth_multiplier) as usize;
        calculated.max(2).min(15) // At least 2, at most 15
    }

    pub async fn gather_context(
        &self,
        conversations: &[ConversationRecord],
        memories: &[models::MemoryRecord],
        current_objective: &Option<ObjectiveDefinition>,
        search_pattern: dto::json::agent_executor::SearchPattern,
        expected_depth: Option<dto::json::agent_executor::SearchDepth>,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        tracing::info!(
            "Starting context gathering for agent {} with objective: {:?}, pattern: {:?}, depth: {:?}",
            self.agent.id,
            current_objective.as_ref().map(|o| &o.primary_goal),
            search_pattern,
            expected_depth
        );

        let max_iterations = self.calculate_max_iterations(search_pattern, expected_depth);
        tracing::info!(
            "Adaptive search depth: max {} iterations for pattern {:?} with depth {:?}",
            max_iterations,
            search_pattern,
            expected_depth
        );

        let mut all_results = Vec::new();
        let mut previous_searches = Vec::new();
        let mut search_metrics = SearchMetrics::new();
        let mut iteration = 1;

        loop {
            // Create enhanced progress data for the agent
            let progress_data = if iteration > 1 {
                Some(self.create_progress_summary(&search_metrics, &previous_searches))
            } else {
                None
            };

            let derivation = self
                .derive_search_strategy(
                    conversations,
                    memories,
                    current_objective,
                    &previous_searches,
                    progress_data.as_ref(),
                    search_pattern,
                    iteration,
                    max_iterations,
                )
                .await?;

            tracing::info!(
                "Iteration {}: Search strategy - Action: {:?}, Query: '{}', Reasoning: {}",
                iteration,
                derivation.next_action,
                derivation.search_query,
                derivation.reasoning
            );

            // Check if agent decided to stop
            if matches!(derivation.next_action, SearchScope::Complete) {
                tracing::info!(
                    "Context gathering complete after {} iterations. Total results: {}",
                    iteration - 1,
                    all_results.len()
                );
                break;
            }

            // PARAMETER VALIDATION: Check for required parameters and retry if missing
            let parameter_error = match &derivation.next_action {
                SearchScope::ReadKnowledgeBaseDocuments => {
                    if derivation.read_document_params.is_none() {
                        Some("ERROR: ReadKnowledgeBaseDocuments requires read_document_params with a valid document_id. You must provide a document_id from a previous ListKnowledgeBaseDocuments search.")
                    } else if let Some(params) = &derivation.read_document_params {
                        if params.document_id.is_empty() {
                            Some("ERROR: ReadKnowledgeBaseDocuments requires a non-empty document_id. Use the document_id from a previous document listing.")
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                SearchScope::ListKnowledgeBaseDocuments => {
                    if derivation.list_documents_params.is_none() {
                        Some("ERROR: ListKnowledgeBaseDocuments requires list_documents_params with page and limit fields. Use: {\"page\": 1, \"limit\": 100}")
                    } else {
                        None
                    }
                }
                _ => None,
            };

            if let Some(error_msg) = parameter_error {
                // Add error message to previous searches for context
                let error_context = json!({
                    "iteration": iteration,
                    "error_message": error_msg,
                    "failed_action": format!("{:?}", derivation.next_action),
                    "failed_query": derivation.search_query
                });
                previous_searches.push(error_context);

                // Retry with the same context but include the error message
                continue;
            }

            let results = match self.execute_search(&derivation).await {
                Ok(results) => {
                    tracing::info!(
                        "Search returned {} results for iteration {}",
                        results.len(),
                        iteration
                    );
                    results
                }
                Err(e) => {
                    tracing::warn!(
                        "Search failed in iteration {}: {}. Adding error context.",
                        iteration,
                        e
                    );
                    // Create an informative result about the failure
                    vec![ContextSearchResult {
                        source: ContextSource::KnowledgeBase { kb_id: 0, document_id: 0 },
                        content: format!(
                            "Failed to execute search '{}': {}. This resource may not exist or is not accessible. Consider alternative search strategies.",
                            derivation.search_query,
                            e
                        ),
                        relevance_score: 0.1,
                        metadata: json!({
                            "error_type": "search_failure",
                            "failed_action": format!("{:?}", derivation.next_action),
                            "failed_query": derivation.search_query,
                            "error_message": e.to_string(),
                            "iteration": iteration,
                            "suggestion": "Try searching by document name or listing available documents first"
                        }),
                    }]
                }
            };

            // Add results to accumulator BEFORE any potential break
            all_results.extend(results.clone());

            // Update metrics for progress tracking
            let current_progress = search_metrics.update(&derivation.search_query, &results);

            let mut forced_stop = false;

            if current_progress.query_similarity_score >= 0.9 {
                forced_stop = true;
            } else if current_progress.new_sources_this_iteration == 0 && iteration >= 3 {
                let recent_low_progress = search_metrics.consecutive_zero_progress >= 2;
                if recent_low_progress {
                    forced_stop = true;
                }
            } else if results.len() >= 15
                && matches!(
                    derivation.next_action,
                    SearchScope::ListKnowledgeBaseDocuments
                )
            {
                forced_stop = true;
            }

            if forced_stop {
                break;
            }

            // Check if this search had an error
            let had_error = results
                .iter()
                .any(|r| r.metadata.get("error_type").is_some());
            let error_info = if had_error {
                results
                    .iter()
                    .find(|r| r.metadata.get("error_type").is_some())
                    .and_then(|r| {
                        Some(json!({
                            "error_message": r.metadata.get("error_message"),
                            "suggestion": r.metadata.get("suggestion"),
                            "failed_action": r.metadata.get("failed_action")
                        }))
                    })
            } else {
                None
            };

            let search_record = json!({
                "iteration": iteration,
                "search_query": derivation.search_query,
                "next_action": format!("{:?}", derivation.next_action),
                "search_mode": format!("{:?}", derivation.filters.search_mode),
                "max_results": derivation.filters.max_results,
                "knowledge_base_ids": derivation.filters.knowledge_base_ids.clone(),
                "boost_keywords": derivation.filters.boost_keywords.clone(),
                "time_range": derivation.filters.time_range.clone(),
                "results_count": results.len(),
                "had_error": had_error,
                "error_info": error_info,
                "progress_metrics": {
                    "new_sources": current_progress.new_sources_this_iteration,
                    "total_unique_sources": current_progress.total_unique_sources,
                    "overlap_percentage": current_progress.result_overlap_percentage,
                    "query_similarity": current_progress.query_similarity_score,
                    "discovery_trend": current_progress.discovery_rate_trend,
                    "information_density_declining": current_progress.information_density_declining,
                }
            });

            previous_searches.push(search_record);

            // Use adaptive max iterations based on pattern and depth
            if iteration >= max_iterations {
                tracing::info!(
                    "Reached max iterations ({}) for pattern {:?} with depth {:?}",
                    max_iterations,
                    search_pattern,
                    expected_depth
                );
                break;
            }

            // Memory management
            if all_results.len() > 1000 {
                all_results.truncate(500);
            }

            iteration += 1;
        }

        let mut deduped_results = self.deduplicate_results(all_results);

        if !previous_searches.is_empty() {
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
                        .get("next_action")
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

    fn create_progress_summary(&self, metrics: &SearchMetrics, searches: &[Value]) -> Value {
        let avg_results = if !searches.is_empty() {
            searches
                .iter()
                .filter_map(|s| s.get("results_count").and_then(|v| v.as_i64()))
                .sum::<i64>() as f32
                / searches.len() as f32
        } else {
            0.0
        };

        let query_repetition_score = metrics
            .query_similarity_scores
            .values()
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(&0.0);

        json!({
            "search_convergence": {
                "total_unique_sources_found": metrics.total_unique_results,
                "discovery_rate_trend": metrics.get_discovery_trend(),
                "information_density_declining": metrics.is_density_declining(),
                "consecutive_low_yields": metrics.consecutive_low_yields,
                "consecutive_duplicates": metrics.consecutive_duplicates,
                "highest_query_similarity": query_repetition_score,
            },
            "effectiveness_metrics": {
                "avg_results_per_iteration": avg_results,
                "total_iterations": searches.len(),
                "search_space_coverage_indicators": {
                    "unique_queries": metrics.query_similarity_scores.len(),
                    "scopes_explored": searches.iter()
                        .filter_map(|s| s.get("next_action").and_then(|v| v.as_str()))
                        .collect::<std::collections::HashSet<_>>()
                        .len(),
                    "modes_used": searches.iter()
                        .filter_map(|s| s.get("search_mode").and_then(|v| v.as_str()))
                        .collect::<std::collections::HashSet<_>>()
                        .len(),
                }
            },
            "loop_detection_signals": {
                "query_similarity_threshold_reached": query_repetition_score > &0.7,
                "diminishing_returns_detected": metrics.is_density_declining(),
                "potential_loop_indicators": {
                    "high_result_overlap": metrics.result_overlap_tracking.len() >= 2 &&
                        metrics.result_overlap_tracking.windows(2).last()
                        .map(|w| w[1].intersection(&w[0]).count() as f32 / w[1].len().max(1) as f32 > 0.8)
                        .unwrap_or(false),
                    "consecutive_failures": metrics.consecutive_low_yields >= 2,
                    "same_results_pattern": metrics.consecutive_duplicates >= 1,
                }
            }
        })
    }

    async fn derive_search_strategy(
        &self,
        conversations: &[ConversationRecord],
        memories: &[models::MemoryRecord],
        current_objective: &Option<ObjectiveDefinition>,
        previous_searches: &[Value],
        progress_data: Option<&Value>,
        search_pattern: dto::json::agent_executor::SearchPattern,
        iteration: usize,
        max_iterations: usize,
    ) -> Result<ContextSearchDerivation, AppError> {
        // Log the last user message
        if let Some(last_user_msg) = conversations
            .iter()
            .rev()
            .find(|c| matches!(c.message_type, ConversationMessageType::UserMessage))
        {
            if let ConversationContent::UserMessage { .. } = &last_user_msg.content {}
        }

        let mut template_data = json!({
            "conversation_history": self.format_conversation_history(conversations),
            "memories": self.format_memories_for_template(memories),
            "current_objective": current_objective,
            "available_knowledge_bases": self.agent.knowledge_bases.clone(),
            "has_previous_searches": !previous_searches.is_empty(),
            "previous_search_count": previous_searches.len(),
            "previous_search_results": previous_searches,
            "search_pattern": format!("{:?}", search_pattern).to_lowercase(),
            "pattern_guidance": self.get_pattern_guidance(search_pattern),
            "current_iteration": iteration,
            "max_iterations": max_iterations,
            "iterations_remaining": max_iterations.saturating_sub(iteration),
        });

        // Add progress data if available
        if let Some(progress) = progress_data {
            template_data["search_progress_analysis"] = progress.clone();
            template_data["has_progress_data"] = json!(true);
        } else {
            template_data["has_progress_data"] = json!(false);
        }

        let request_body =
            render_template_with_prompt(AgentTemplates::CONTEXT_SEARCH_DERIVATION, template_data)
                .map_err(|e| AppError::Internal(format!("Failed to render template: {e}")))?;

        let (derivation, _) = self
            .create_gemini_client()?
            .generate_structured_content::<ContextSearchDerivation>(request_body)
            .await?;

        Ok(derivation)
    }

    async fn execute_search(
        &self,
        derivation: &ContextSearchDerivation,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let filters = self.convert_filters(&derivation.filters)?;
        let max_results = derivation.filters.max_results as usize;
        let query = &derivation.search_query;

        tracing::info!(
            "Executing search - Type: {:?}, Query: '{}', Max Results: {}, Filters: {:?}",
            derivation.next_action,
            query,
            max_results,
            derivation.filters
        );

        match &derivation.next_action {
            SearchScope::KnowledgeBase => {
                tracing::info!(
                    "KB Search - Mode: {:?}, KB IDs: {:?}, Boost Keywords: {:?}",
                    filters.search_mode,
                    derivation.filters.knowledge_base_ids,
                    filters.boost_keywords
                );
                let kb_ids = derivation.filters.knowledge_base_ids.as_ref().map(|ids| {
                    ids.iter()
                        .filter_map(|id| id.parse::<i64>().map_err(|_| {}).ok())
                        .collect::<Vec<i64>>()
                });

                self.execute_knowledge_base_search(query, kb_ids, max_results, &filters)
                    .await
            }
            SearchScope::ListKnowledgeBaseDocuments => {
                tracing::info!(
                    "List KB Documents - Params: {:?}",
                    derivation.list_documents_params
                );
                self.list_knowledge_base_documents(derivation).await
            }
            SearchScope::ReadKnowledgeBaseDocuments => {
                tracing::info!(
                    "Read KB Document - Document ID: {:?}, Chunk Range: {:?}",
                    derivation
                        .read_document_params
                        .as_ref()
                        .map(|p| &p.document_id),
                    derivation
                        .read_document_params
                        .as_ref()
                        .and_then(|p| p.chunk_range.as_ref())
                );
                self.read_knowledge_base_documents(derivation).await
            }
            SearchScope::Conversations => {
                tracing::info!(
                    "Conversation Search - Query: '{}', Max Results: {}",
                    query,
                    max_results
                );
                self.search_conversations(query, max_results).await
            }
            SearchScope::Complete => Ok(Vec::new()),
        }
    }

    fn convert_filters(&self, llm_filters: &LLMFilters) -> Result<ContextFilters, AppError> {
        let search_mode = match &llm_filters.search_mode {
            LLMSearchMode::Semantic => SearchMode::Vector,
            LLMSearchMode::Keyword => SearchMode::FullText,
            LLMSearchMode::Hybrid => {
                let vector_weight = 0.7;
                let text_weight = 0.3;

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

        tracing::debug!(
            "Executing KB search - Query: '{}', KB IDs: {:?}, Mode: {:?}",
            query,
            kb_ids,
            filters.search_mode
        );

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
                    "document_id": r.document_id.to_string(),  // Store as string to preserve Snowflake ID
                    "knowledge_base_id": r.knowledge_base_id.to_string(),  // Store as string
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
        tracing::debug!(
            "Vector Search - Query: '{}', KB IDs: {:?}, Max Results: {}, Embedding Dim: {}",
            query,
            kb_ids,
            max_results,
            query_embedding.len()
        );

        let results = SearchKnowledgeBaseEmbeddingsCommand::new(
            kb_ids.to_vec(),
            query_embedding.to_vec(),
            max_results as u64,
        )
        .execute(&self.app_state)
        .await?;

        tracing::debug!("Vector search returned {} results", results.len());

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
                    "document_id": r.document_id.to_string(),  // Store as string to preserve Snowflake ID
                    "knowledge_base_id": r.knowledge_base_id.to_string(),  // Store as string
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
                    "document_id": r.document_id.to_string(),  // Store as string to preserve Snowflake ID
                    "knowledge_base_id": r.knowledge_base_id.to_string(),  // Store as string
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
                .filter_map(|id| id.parse::<i64>().map_err(|_| {}).ok())
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
        if list_params.page < 1 {}
        if list_params.limit < 1 || list_params.limit > 200 {}

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
                    AppError::Internal(format!("Failed to fetch documents from KB {kb_id}: {e}"))
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
                        "document_id": doc.id.to_string(),  // CRITICAL: Store as string to preserve Snowflake ID precision
                        "document_title": doc.title,
                        "knowledge_base_id": kb_id.to_string(),  // Also store KB ID as string
                        "created_at": doc.created_at,
                        "page": page,
                    }),
                });
            }
        }

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
            b_created.cmp(a_created)
        });

        all_documents.truncate(limit as usize);

        let any_has_next_page = kb_has_next_page.values().any(|&has_next| has_next);

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
                        "knowledge_base_ids": kb_ids.iter().map(|id| id.to_string()).collect::<Vec<String>>(),
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

        // Parse comma-separated document IDs
        let document_ids_str = read_params.document_id.trim();
        let document_id_strings: Vec<&str> = if document_ids_str.contains(',') {
            document_ids_str.split(',').map(|s| s.trim()).collect()
        } else {
            vec![document_ids_str]
        };

        tracing::info!(
            "Attempting to parse document_ids: {:?}",
            document_id_strings
        );

        // Parse and validate all document IDs
        let mut document_ids = Vec::new();
        let mut invalid_ids = Vec::new();

        for id_str in document_id_strings {
            match id_str.parse::<i64>() {
                Ok(id) => document_ids.push(id),
                Err(_) => invalid_ids.push(id_str.to_string()),
            }
        }

        // If there are invalid IDs, include them in the error response
        if !invalid_ids.is_empty() {
            let error_msg = format!(
                "Invalid document ID format: {}. All IDs must be valid numbers.",
                invalid_ids.join(", ")
            );
            tracing::error!("{}", error_msg);
        }

        // If no valid IDs were found, return error
        if document_ids.is_empty() {
            return Ok(vec![ContextSearchResult {
                source: ContextSource::KnowledgeBase {
                    kb_id: 0,
                    document_id: 0,
                },
                content: format!(
                    "No valid document IDs found in: '{}'. All IDs must be valid numbers.",
                    read_params.document_id
                ),
                relevance_score: 0.1,
                metadata: json!({
                    "error": "invalid_document_ids",
                    "provided_ids": read_params.document_id,
                    "invalid_ids": invalid_ids,
                    "message": "All document IDs must be valid numbers"
                }),
            }]);
        }

        let limit_per_document = read_params.limit.unwrap_or(10) as usize;
        let mut all_results = Vec::new();
        let mut successful_docs = Vec::new();
        let mut failed_docs = Vec::new();

        for document_id in &document_ids {
            tracing::info!("Fetching chunks for document_id: {}", document_id);

            let mut query = GetDocumentChunksQuery::new(*document_id);

            if let Some(keywords) = &read_params.keywords {
                query = query.with_keywords(keywords.clone());
            }

            if let Some(range) = &read_params.chunk_range {
                query = query.with_chunk_range(range.start, range.end);
            }

            query = query.with_limit(limit_per_document);

            match query.execute(&self.app_state).await {
                Ok(chunks) => {
                    if chunks.is_empty() {
                        tracing::warn!(
                            "No chunks found for document_id {}. Document may be empty or not properly indexed.",
                            document_id
                        );
                        failed_docs.push(format!("{} (no chunks)", document_id));

                        // Add informative result for empty document
                        all_results.push(ContextSearchResult {
                            source: ContextSource::KnowledgeBase {
                                kb_id: 0,
                                document_id: *document_id
                            },
                            content: format!(
                                "Document with ID {} exists but has no content chunks. It may be empty or not properly indexed.",
                                document_id
                            ),
                            relevance_score: 0.1,
                            metadata: json!({
                                "error": "no_chunks",
                                "document_id": document_id.to_string(),
                                "message": "Document has no content chunks"
                            }),
                        });
                    } else {
                        successful_docs.push(document_id.to_string());
                        let kb_id = chunks.first().map(|c| c.knowledge_base_id).unwrap_or(0);

                        // Add all chunks from this document
                        for chunk in chunks {
                            all_results.push(ContextSearchResult {
                                source: ContextSource::KnowledgeBase {
                                    kb_id,
                                    document_id: *document_id,
                                },
                                content: chunk.content,
                                relevance_score: 1.0,
                                metadata: json!({
                                    "chunk_index": chunk.chunk_index,
                                    "document_id": document_id.to_string(),
                                    "knowledge_base_id": kb_id.to_string(),
                                }),
                            });
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to fetch chunks for document_id {}: {}",
                        document_id,
                        e
                    );
                    failed_docs.push(format!("{} (fetch error)", document_id));

                    // Add error result for this document
                    all_results.push(ContextSearchResult {
                        source: ContextSource::KnowledgeBase { kb_id: 0, document_id: *document_id },
                        content: format!(
                            "Failed to fetch document with ID {}: {}. Document may not exist or access is denied.",
                            document_id,
                            e
                        ),
                        relevance_score: 0.1,
                        metadata: json!({
                            "error": "fetch_failed",
                            "document_id": document_id.to_string(),
                            "error_message": e.to_string(),
                            "message": "Document fetch failed"
                        }),
                    });
                }
            }
        }

        // Add summary result at the beginning if multiple documents were requested
        if document_ids.len() > 1 {
            let summary_content = if successful_docs.is_empty() && failed_docs.is_empty() {
                format!(
                    "No valid documents found from the provided IDs: {}",
                    read_params.document_id
                )
            } else {
                let mut summary_parts = Vec::new();

                if !successful_docs.is_empty() {
                    summary_parts.push(format!(
                        "Successfully loaded {} documents: {}",
                        successful_docs.len(),
                        successful_docs.join(", ")
                    ));
                }

                if !failed_docs.is_empty() {
                    summary_parts.push(format!(
                        "Failed to load {} documents: {}",
                        failed_docs.len(),
                        failed_docs.join(", ")
                    ));
                }

                if !invalid_ids.is_empty() {
                    summary_parts.push(format!("Invalid document IDs: {}", invalid_ids.join(", ")));
                }

                format!("Multi-document read results: {}", summary_parts.join("; "))
            };

            all_results.insert(
                0,
                ContextSearchResult {
                    source: ContextSource::System,
                    content: summary_content,
                    relevance_score: 1.0,
                    metadata: json!({
                        "is_multi_document_summary": true,
                        "requested_documents": document_ids.len(),
                        "successful_documents": successful_docs.len(),
                        "failed_documents": failed_docs.len(),
                        "invalid_ids": invalid_ids,
                        "successful_ids": successful_docs,
                        "failed_ids": failed_docs,
                    }),
                },
            );
        }

        Ok(all_results)
    }

    fn deduplicate_results(&self, results: Vec<ContextSearchResult>) -> Vec<ContextSearchResult> {
        let mut unique_results = Vec::new();
        let mut seen_items = HashSet::new();

        for result in results {
            let unique_key = match &result.source {
                ContextSource::KnowledgeBase { kb_id, document_id } => {
                    let chunk_index = result
                        .metadata
                        .get("chunk_index")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    format!("kb_{kb_id}_doc_{document_id}_chunk_{chunk_index}")
                }
                ContextSource::Conversation { conversation_id } => {
                    format!("conversation_{conversation_id}")
                }
                ContextSource::System => {
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
            ConversationContent::UserMessage { message, .. } => message.clone(),
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
            Some("gemini-2.5-flash-lite".to_string()),
        ))
    }

    fn format_memories_for_template(&self, memories: &[models::MemoryRecord]) -> Vec<Value> {
        memories
            .iter()
            .map(|mem| {
                json!({
                    "content": mem.content.clone(),
                    "importance": mem.base_temporal_score,
                    "category": format!("{:?}", mem.memory_category),
                    "access_count": mem.access_count,
                    "last_accessed": mem.last_accessed_at,
                })
            })
            .collect()
    }

    async fn search_conversations(
        &self,
        _query: &str,
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
                    ConversationContent::UserMessage { message, .. } => message.clone(),
                    ConversationContent::AgentResponse { response, .. } => response.clone(),
                    ConversationContent::AssistantAcknowledgment {
                        acknowledgment_message,
                        ..
                    } => {
                        format!("Acknowledgment: {acknowledgment_message}")
                    }
                    ConversationContent::ActionExecutionResult { task_execution, .. } => {
                        format!("Task execution: {task_execution}")
                    }
                    ConversationContent::ContextResults {
                        query,
                        result_count,
                        ..
                    } => {
                        format!("Context search for '{query}' found {result_count} results")
                    }
                    _ => format!("{:?}", conv.message_type),
                };

                ContextSearchResult {
                    source: ContextSource::Conversation {
                        conversation_id: conv.id,
                    },
                    content,
                    relevance_score: 1.0 - (idx as f64 * 0.01),
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
