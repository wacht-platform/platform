use crate::filesystem::knowledge_base_mount_name;
use commands::{GenerateEmbeddingCommand, SearchKnowledgeBaseEmbeddingsCommand};
use common::error::AppError;
use common::state::AppState;
use dto::json::agent_executor::{
    ContextChunkMatch, ContextHints, LocalKnowledgeSearchType, RecommendedFile, SearchConclusion,
};
use dto::json::{ContextFilters, ContextSearchResult, ContextSource, SearchMode};
use models::AiAgentWithFeatures;
use queries::{
    FullTextSearchKnowledgeBaseQuery, GetDocumentChunksQuery, HybridSearchKnowledgeBaseQuery,
};
use serde_json::json;
use std::collections::HashSet;

const RELEVANCE_SCORE_DIVISOR: f32 = 2.0;

pub struct ContextOrchestrator {
    ctx: std::sync::Arc<crate::execution_context::ExecutionContext>,
}

impl ContextOrchestrator {
    pub fn new(ctx: std::sync::Arc<crate::execution_context::ExecutionContext>) -> Self {
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
        include_associated_chunks: bool,
        max_associated_chunks_per_document: usize,
        max_query_rewrites: usize,
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
                mode: Some("search_local_knowledge".to_string()),
                search_method: Some(format!("{:?}", search_type).to_lowercase()),
                requested_output: None,
                extracted_output: None,
                chunk_matches: Some(vec![]),
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

        let query_rewrites = build_query_rewrites(query, max_query_rewrites.clamp(1, 6));
        let mut results = Vec::new();
        let mut attempted_queries = Vec::new();

        for rewritten_query in &query_rewrites {
            attempted_queries.push(rewritten_query.clone());
            let iteration_results = self
                .execute_knowledge_base_search(
                    rewritten_query,
                    Some(kb_ids.clone()),
                    filters.max_results,
                    &filters,
                )
                .await?;
            results.extend(iteration_results);

            if results.len() >= filters.max_results * 2 {
                break;
            }
        }

        let mut recommended_files: Vec<RecommendedFile> = Vec::new();
        let mut top_documents: Vec<(i64, String, String)> = Vec::new();
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

                    top_documents.push((*document_id, path.clone(), doc_title.clone()));
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

        if include_associated_chunks {
            let keyword_terms: Vec<String> = query
                .split_whitespace()
                .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric()))
                .filter(|s| s.len() >= 3)
                .take(6)
                .map(|s| s.to_string())
                .collect();

            for (document_id, path, title) in top_documents.iter().take(5) {
                let mut query_builder = GetDocumentChunksQuery::new(*document_id)
                    .with_limit(max_associated_chunks_per_document.clamp(1, 10));

                if !keyword_terms.is_empty() {
                    query_builder = query_builder.with_keywords(keyword_terms.clone());
                }

                if let Ok(chunks) = query_builder
                    .execute_with(self.app_state().db_router.writer())
                    .await
                {
                    for chunk in chunks {
                        let chunk_key = format!(
                            "{}_{}_{}",
                            chunk.knowledge_base_id, document_id, chunk.chunk_index
                        );
                        if seen_chunks.contains(&chunk_key) {
                            continue;
                        }

                        seen_chunks.insert(chunk_key);
                        chunk_matches.push(ContextChunkMatch {
                            path: path.clone(),
                            document_title: title.clone(),
                            document_id: document_id.to_string(),
                            knowledge_base_id: chunk.knowledge_base_id.to_string(),
                            chunk_index: chunk.chunk_index,
                            relevance_score: 0.5,
                            excerpt: truncate_for_excerpt(&chunk.content, 400),
                            source: "associated".to_string(),
                        });
                    }
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
            mode: Some("search_local_knowledge".to_string()),
            search_method: Some(format!("{:?}", search_type).to_lowercase()),
            requested_output: None,
            extracted_output: None,
            chunk_matches: Some(chunk_matches),
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
        let gemini_api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY is not set".to_string()))?;
        let gemini_model = std::env::var("GEMINI_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "models/gemini-embedding-001".to_string());
        let gemini_client = reqwest::Client::new();

        GenerateEmbeddingCommand::new(query.to_string())
            .with_task_type("RETRIEVAL_QUERY".to_string())
            .execute_with(&gemini_client, &gemini_api_key, &gemini_model)
            .await
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
            deployment_id: self.agent().deployment_id,
            max_results: max_results as i32,
        }
        .execute_with(self.app_state().db_router.writer())
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
        let results = SearchKnowledgeBaseEmbeddingsCommand::new(
            kb_ids.to_vec(),
            query_embedding.to_vec(),
            max_results as u64,
        )
        .execute_with(self.app_state().db_router.writer())
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

        let results = HybridSearchKnowledgeBaseQuery {
            knowledge_base_ids: kb_ids.to_vec(),
            query_text,
            query_embedding: query_embedding.to_vec(),
            vector_weight: vector_weight as f64,
            text_weight: text_weight as f64,
            deployment_id: self.agent().deployment_id,
            max_results: max_results as i32,
        }
        .execute_with(self.app_state().db_router.writer())
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

fn build_query_rewrites(query: &str, max_rewrites: usize) -> Vec<String> {
    let mut rewrites = Vec::new();
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return vec!["knowledge search".to_string()];
    }
    rewrites.push(trimmed.to_string());

    let mut keywords: Vec<String> = trimmed
        .split_whitespace()
        .map(|s| {
            s.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|s| s.len() >= 3)
        .collect();
    keywords.sort();
    keywords.dedup();

    if !keywords.is_empty() {
        rewrites.push(
            keywords
                .iter()
                .take(6)
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    if keywords.len() >= 2 {
        rewrites.push(
            keywords
                .iter()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    if keywords.len() >= 4 {
        rewrites.push(
            keywords
                .iter()
                .skip(2)
                .take(4)
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
        );
    }

    rewrites
        .into_iter()
        .filter(|q| !q.trim().is_empty())
        .take(max_rewrites)
        .collect()
}
