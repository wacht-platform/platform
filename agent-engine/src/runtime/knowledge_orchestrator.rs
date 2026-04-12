use crate::filesystem::knowledge_base_mount_name;
use commands::GenerateEmbeddingCommand;
use common::state::AppState;
use common::{
    error::AppError, search_full_text_in_table, search_hybrid_in_table, search_vector_in_table,
};
use dto::json::agent_executor::{
    ContextChunkMatch, ContextHints, LocalKnowledgeSearchType, RecommendedFile, SearchConclusion,
};
use dto::json::{ContextFilters, ContextSearchResult, ContextSource, SearchMode};
use models::AiAgentWithFeatures;
use serde_json::json;
use std::collections::HashSet;

const RELEVANCE_SCORE_DIVISOR: f32 = 2.0;

pub struct KnowledgeOrchestrator {
    ctx: std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
}

impl KnowledgeOrchestrator {
    pub fn new(
        ctx: std::sync::Arc<crate::runtime::thread_execution_context::ThreadExecutionContext>,
    ) -> Self {
        Self { ctx }
    }

    #[inline]
    fn app_state(&self) -> &AppState {
        &self.ctx.app_state
    }

    #[inline]
    fn agent(&self) -> &AiAgentWithFeatures {
        &self.ctx.agent
    }

    pub async fn gather_local_knowledge_hints(
        &self,
        query: &str,
        search_type: LocalKnowledgeSearchType,
        knowledge_base_ids: Option<Vec<String>>,
        max_results: usize,
    ) -> Result<ContextHints, AppError> {
        let kb_ids: Vec<i64> = if let Some(ids) = knowledge_base_ids {
            ids.into_iter()
                .filter_map(|id| id.parse::<i64>().ok())
                .collect()
        } else {
            self.agent()
                .knowledge_bases
                .iter()
                .map(|kb| kb.id)
                .collect()
        };

        if kb_ids.is_empty() {
            return Ok(ContextHints {
                recommended_files: vec![],
                search_summary: "No local knowledge bases are available for search.".to_string(),
                search_conclusion: SearchConclusion::NothingFound,
                search_terms_used: vec![query.to_string()],
                knowledge_bases_searched: vec![],
                requested_output: None,
                extracted_output: None,
            });
        }

        let search_mode = match search_type {
            LocalKnowledgeSearchType::Semantic => SearchMode::Vector,
            LocalKnowledgeSearchType::Keyword => SearchMode::FullText,
        };

        let filters = ContextFilters {
            max_results: max_results.clamp(1, 50),
            time_range: None,
            search_mode,
            boost_keywords: None,
        };

        let mut results = Vec::new();
        let attempted_queries = vec![query.trim().to_string()];
        let iteration_results = self
            .execute_knowledge_base_search(
                query,
                Some(kb_ids.clone()),
                filters.max_results,
                &filters,
            )
            .await?;
        results.extend(iteration_results);

        let mut recommended_files: Vec<RecommendedFile> = Vec::new();
        let mut seen_documents: HashSet<String> = HashSet::new();
        let mut kb_names: HashSet<String> = HashSet::new();
        let mut chunk_matches: Vec<ContextChunkMatch> = Vec::new();
        let mut seen_chunks: HashSet<String> = HashSet::new();

        for result in &results {
            if let ContextSource::KnowledgeBase { kb_id, document_id } = &result.source {
                let kb_name = self
                    .agent()
                    .knowledge_bases
                    .iter()
                    .find(|kb| kb.id == *kb_id)
                    .map(|kb| kb.name.clone())
                    .unwrap_or_else(|| format!("kb_{}", kb_id));
                let kb_mount_name = knowledge_base_mount_name(&kb_id.to_string(), &kb_name);
                kb_names.insert(kb_name.clone());

                let doc_title = result
                    .metadata
                    .get("document_title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("document")
                    .to_string();
                let path = format!("/knowledge/{}/{}", kb_mount_name, doc_title);

                let doc_key = format!("{}_{}", kb_id, document_id);
                if !seen_documents.contains(&doc_key) {
                    seen_documents.insert(doc_key);
                    let sample = if result.content.len() > 200 {
                        Some(format!("{}...", &result.content[..200]))
                    } else if !result.content.is_empty() {
                        Some(result.content.clone())
                    } else {
                        None
                    };

                    recommended_files.push(RecommendedFile {
                        path: path.clone(),
                        document_title: doc_title.clone(),
                        relevance_score: result.relevance_score as f32,
                        reason: format!(
                            "Matched local {} search for '{}'",
                            format!("{:?}", search_type).to_lowercase(),
                            query
                        ),
                        sample_text: sample,
                    });
                }

                let chunk_index = result
                    .metadata
                    .get("chunk_index")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
                let chunk_key = format!("{}_{}_{}", kb_id, document_id, chunk_index);

                if !seen_chunks.contains(&chunk_key) {
                    seen_chunks.insert(chunk_key);
                    chunk_matches.push(ContextChunkMatch {
                        path,
                        document_title: doc_title,
                        document_id: document_id.to_string(),
                        knowledge_base_id: kb_id.to_string(),
                        chunk_index,
                        relevance_score: result.relevance_score as f32,
                        excerpt: truncate_for_excerpt(&result.content, 400),
                        source: "matched".to_string(),
                    });
                }
            }
        }

        recommended_files.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        recommended_files.truncate(10);

        chunk_matches.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        chunk_matches.truncate(30);

        let conclusion = if recommended_files.is_empty() {
            SearchConclusion::NothingFound
        } else if recommended_files.len() >= 3 {
            SearchConclusion::FoundRelevant
        } else {
            SearchConclusion::PartialMatch
        };

        Ok(ContextHints {
            recommended_files,
            search_summary: format!(
                "Local knowledge {} search for '{}' ran {} retrieval pass(es), returned {} candidate documents and {} chunks.",
                format!("{:?}", search_type).to_lowercase(),
                query,
                attempted_queries.len(),
                seen_documents.len(),
                chunk_matches.len()
            ),
            search_conclusion: conclusion,
            search_terms_used: attempted_queries,
            knowledge_bases_searched: kb_names.into_iter().collect(),
            requested_output: None,
            extracted_output: None,
        })
    }

    async fn execute_knowledge_base_search(
        &self,
        query: &str,
        kb_ids: Option<Vec<i64>>,
        max_results: usize,
        filters: &ContextFilters,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let kb_ids = kb_ids.unwrap_or_else(|| {
            self.agent()
                .knowledge_bases
                .iter()
                .map(|kb| kb.id)
                .collect()
        });

        let results = match &filters.search_mode {
            SearchMode::FullText => {
                let enhanced_query = if let Some(keywords) = &filters.boost_keywords {
                    format!("{} {}", query, keywords.join(" "))
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
        GenerateEmbeddingCommand::new(query.to_string())
            .for_retrieval_query()
            .for_deployment(self.agent().deployment_id)
            .execute_with_deps(self.app_state())
            .await
    }

    async fn search_kb_fulltext(
        &self,
        kb_ids: &[i64],
        query: &str,
        max_results: usize,
    ) -> Result<Vec<ContextSearchResult>, AppError> {
        let Some(table) = self.ctx.get_kb_table().await? else {
            return Ok(Vec::new());
        };
        let results = search_full_text_in_table(&table, kb_ids, query, max_results).await?;

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
                    "document_id": r.document_id.to_string(),
                    "knowledge_base_id": r.knowledge_base_id.to_string(),
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
        let Some(table) = self.ctx.get_kb_table().await? else {
            return Ok(Vec::new());
        };
        let results = search_vector_in_table(&table, kb_ids, query_embedding, max_results).await?;

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
                    "document_id": r.document_id.to_string(),
                    "knowledge_base_id": r.knowledge_base_id.to_string(),
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

        let Some(table) = self.ctx.get_kb_table().await? else {
            return Ok(Vec::new());
        };
        let results =
            search_hybrid_in_table(&table, kb_ids, &query_text, query_embedding, max_results)
                .await?;

        Ok(results
            .into_iter()
            .map(|r| ContextSearchResult {
                source: ContextSource::KnowledgeBase {
                    kb_id: r.knowledge_base_id,
                    document_id: r.document_id,
                },
                content: r.content,
                relevance_score: r.combined_score / RELEVANCE_SCORE_DIVISOR as f64,
                metadata: json!({
                    "document_id": r.document_id.to_string(),
                    "knowledge_base_id": r.knowledge_base_id.to_string(),
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
}

fn truncate_for_excerpt(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }

    let mut out = String::new();
    for ch in input.chars().take(max_chars) {
        out.push(ch);
    }
    out.push_str("...");
    out
}
