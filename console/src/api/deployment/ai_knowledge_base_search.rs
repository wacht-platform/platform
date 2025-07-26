use axum::extract::{Path, Query, State};

use crate::{
    application::{AppError, HttpState, response::ApiResult},
    core::{
        commands::{Command, GenerateEmbeddingCommand, SearchKnowledgeBaseEmbeddingsCommand},
        dto::json::ai_knowledge_base::{
            KnowledgeBaseSearchResult, SearchKnowledgeBaseQuery, SearchKnowledgeBaseResponse,
        },
        queries::{Query as QueryTrait, ai_knowledge_base::GetAiKnowledgeBaseByIdQuery},
    },
};

pub async fn search_knowledge_base(
    Path(deployment_id): Path<i64>,
    Query(params): Query<SearchKnowledgeBaseQuery>,
    State(app_state): State<HttpState>,
) -> ApiResult<SearchKnowledgeBaseResponse> {
    let limit = params.limit.unwrap_or(10).min(100);

    let query_embedding = GenerateEmbeddingCommand::new(params.query.clone())
        .execute(&app_state)
        .await?;

    let results = if let Some(kb_id) = params.knowledge_base_id {
        let _kb = GetAiKnowledgeBaseByIdQuery::new(deployment_id, kb_id)
            .execute(&app_state)
            .await
            .map_err(|_| AppError::NotFound("Knowledge base not found".to_string()))?;

        SearchKnowledgeBaseEmbeddingsCommand::new(vec![kb_id], query_embedding, limit)
            .execute(&app_state)
            .await?
    } else {
        return Err(AppError::BadRequest(
            "Please specify a knowledge_base_id parameter for search".to_string(),
        )
        .into());
    };

    let search_results: Vec<KnowledgeBaseSearchResult> = results
        .into_iter()
        .map(|r| KnowledgeBaseSearchResult {
            id: format!("{}-{}", r.document_id, r.chunk_index),
            content: r.content,
            score: r.score as f32,
            knowledge_base_id: Some(r.knowledge_base_id.to_string()),
            title: None,
            file_type: None,
            chunk_index: Some(r.chunk_index as i64),
        })
        .collect();

    let total_results = search_results.len();

    Ok(SearchKnowledgeBaseResponse {
        results: search_results,
        total_results,
        query: params.query,
    }
    .into())
}

/// Search within a specific knowledge base
pub async fn search_specific_knowledge_base(
    Path((deployment_id, knowledge_base_id)): Path<(i64, i64)>,
    Query(params): Query<SearchKnowledgeBaseQuery>,
    State(app_state): State<HttpState>,
) -> ApiResult<SearchKnowledgeBaseResponse> {
    let _kb = GetAiKnowledgeBaseByIdQuery::new(deployment_id, knowledge_base_id)
        .execute(&app_state)
        .await
        .map_err(|_| AppError::NotFound("Knowledge base not found".to_string()))?;

    let limit = params.limit.unwrap_or(10).min(100);

    let query_embedding = GenerateEmbeddingCommand::new(params.query.clone())
        .execute(&app_state)
        .await?;

    let results =
        SearchKnowledgeBaseEmbeddingsCommand::new(vec![knowledge_base_id], query_embedding, limit)
            .execute(&app_state)
            .await?;

    let search_results: Vec<KnowledgeBaseSearchResult> = results
        .into_iter()
        .map(|r| KnowledgeBaseSearchResult {
            id: format!("{}-{}", r.document_id, r.chunk_index),
            content: r.content,
            score: r.score as f32,
            knowledge_base_id: Some(r.knowledge_base_id.to_string()),
            title: None,
            file_type: None,
            chunk_index: Some(r.chunk_index as i64),
        })
        .collect();

    let total_results = search_results.len();

    Ok(SearchKnowledgeBaseResponse {
        results: search_results,
        total_results,
        query: params.query,
    }
    .into())
}
