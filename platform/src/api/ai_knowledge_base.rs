use crate::middleware::RequireDeployment;
use axum::{
    extract::{Json, Multipart, Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::api::multipart::MultipartPayload;
use crate::application::{
    AppError,
    response::{ApiResult, PaginatedResponse},
};
use common::state::AppState;

use commands::{
    AttachKnowledgeBaseToAgentCommand, Command, CreateAiKnowledgeBaseCommand,
    DeleteAiKnowledgeBaseCommand, DeleteKnowledgeBaseDocumentCommand,
    DetachKnowledgeBaseFromAgentCommand, UpdateAiKnowledgeBaseCommand,
    UploadKnowledgeBaseDocumentCommand,
};
use dto::{
    json::ai_knowledge_base::{
        CreateKnowledgeBaseRequest, GetDocumentsQuery, KnowledgeBaseResponse,
        UpdateKnowledgeBaseRequest,
    },
    query::deployment::GetKnowledgeBasesQuery,
};
use models::{AiKnowledgeBase, AiKnowledgeBaseDocument, AiKnowledgeBaseWithDetails};
use queries::{
    GetAgentKnowledgeBasesQuery, GetAiKnowledgeBaseByIdQuery,
    GetAiKnowledgeBasesQuery as GetKnowledgeBasesQueryCore, GetKnowledgeBaseDocumentsQuery,
    Query as QueryTrait,
};

// Unified parameter extraction for knowledge base routes
#[derive(Deserialize)]
pub struct KnowledgeBaseParams {
    pub kb_id: i64,
}

// For document-specific routes that need both kb_id and document_id
#[derive(Deserialize)]
pub struct DocumentParams {
    pub kb_id: i64,
    pub document_id: i64,
}

#[derive(Deserialize)]
pub struct AgentParams {
    pub agent_id: i64,
}

#[derive(Deserialize)]
pub struct AgentKnowledgeBaseParams {
    pub agent_id: i64,
    pub kb_id: i64,
}

fn sanitize_upload_filename(name: &str) -> Option<String> {
    let mut out = String::with_capacity(name.len());
    let mut prev_underscore = false;

    for ch in name.chars() {
        let is_allowed = ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-';
        if is_allowed {
            out.push(ch);
            prev_underscore = false;
        } else if !prev_underscore {
            out.push('_');
            prev_underscore = true;
        }
    }

    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub async fn get_ai_knowledge_bases(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<GetKnowledgeBasesQuery>,
) -> ApiResult<KnowledgeBaseResponse> {
    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);

    let mut query_builder = GetKnowledgeBasesQueryCore::new(deployment_id, limit + 1, offset);

    if let Some(search) = query.search {
        query_builder = query_builder.with_search(search);
    }

    let mut knowledge_bases = query_builder
        .execute(&app_state)
        .await
        .map_err(|e| AppError::from(e))?;

    let has_more = knowledge_bases.len() > limit;
    if has_more {
        knowledge_bases.pop();
    }

    Ok(KnowledgeBaseResponse {
        data: knowledge_bases,
        has_more,
    }
    .into())
}

pub async fn create_ai_knowledge_base(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateKnowledgeBaseRequest>,
) -> ApiResult<AiKnowledgeBase> {
    let configuration = request.configuration.unwrap_or(serde_json::json!({}));

    let knowledge_base = CreateAiKnowledgeBaseCommand::new(
        deployment_id,
        request.name,
        request.description,
        configuration,
    )
    .execute(&app_state)
    .await?;
    Ok(knowledge_base.into())
}

pub async fn get_ai_knowledge_base_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<KnowledgeBaseParams>,
) -> ApiResult<AiKnowledgeBaseWithDetails> {
    let knowledge_base = GetAiKnowledgeBaseByIdQuery::new(deployment_id, params.kb_id)
        .execute(&app_state)
        .await
        .map_err(AppError::from)?;
    Ok(knowledge_base.into())
}

pub async fn get_agent_knowledge_bases(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
) -> ApiResult<PaginatedResponse<AiKnowledgeBaseWithDetails>> {
    let knowledge_bases = GetAgentKnowledgeBasesQuery::new(deployment_id, params.agent_id)
        .execute(&app_state)
        .await?;
    Ok(PaginatedResponse::from(knowledge_bases).into())
}

pub async fn attach_knowledge_base_to_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentKnowledgeBaseParams>,
) -> ApiResult<()> {
    AttachKnowledgeBaseToAgentCommand::new(deployment_id, params.agent_id, params.kb_id)
        .execute(&app_state)
        .await?;
    Ok(().into())
}

pub async fn detach_knowledge_base_from_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentKnowledgeBaseParams>,
) -> ApiResult<()> {
    DetachKnowledgeBaseFromAgentCommand::new(deployment_id, params.agent_id, params.kb_id)
        .execute(&app_state)
        .await?;
    Ok(().into())
}

pub async fn update_ai_knowledge_base(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<KnowledgeBaseParams>,
    Json(request): Json<UpdateKnowledgeBaseRequest>,
) -> ApiResult<AiKnowledgeBase> {
    let mut command = UpdateAiKnowledgeBaseCommand::new(deployment_id, params.kb_id);

    if let Some(name) = request.name {
        command = command.with_name(name);
    }

    if let Some(description) = request.description {
        command = command.with_description(Some(description));
    }

    if let Some(configuration) = request.configuration {
        command = command.with_configuration(configuration);
    }

    let knowledge_base = command.execute(&app_state).await?;
    Ok(knowledge_base.into())
}

pub async fn delete_ai_knowledge_base(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<KnowledgeBaseParams>,
) -> ApiResult<()> {
    DeleteAiKnowledgeBaseCommand::new(deployment_id, params.kb_id)
        .execute(&app_state)
        .await?;
    Ok(().into())
}

pub async fn upload_knowledge_base_document(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<KnowledgeBaseParams>,
    multipart: Multipart,
) -> ApiResult<AiKnowledgeBaseDocument> {
    let mut title: Option<String> = None;
    let mut description: Option<String> = None;
    let mut file_content: Vec<u8> = Vec::new();
    let mut file_name: Option<String> = None;
    let mut file_type: Option<String> = None;

    let payload = MultipartPayload::parse(multipart).await?;

    for field in payload.fields() {
        match field.name.as_str() {
            "title" => {
                title = Some(field.text()?);
            }
            "description" => {
                description = Some(field.text()?);
            }
            "file" => {
                file_name = field.file_name.clone();
                file_type = field.content_type.clone();
                file_content = field.bytes.clone();
            }
            _ => {}
        }
    }

    let file_name = file_name.ok_or((StatusCode::BAD_REQUEST, "File is required".to_string()))?;
    let file_name = sanitize_upload_filename(&file_name)
        .ok_or((StatusCode::BAD_REQUEST, "Invalid filename".to_string()))?;
    let file_type = file_type.unwrap_or("application/octet-stream".to_string());

    let title = title.unwrap_or_else(|| {
        file_name
            .split('.')
            .next()
            .unwrap_or(&file_name)
            .to_string()
    });

    if file_content.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "File content is empty".to_string()).into());
    }

    GetAiKnowledgeBaseByIdQuery::new(deployment_id, params.kb_id)
        .execute(&app_state)
        .await
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                "Knowledge base not found".to_string(),
            )
        })?;

    let document = UploadKnowledgeBaseDocumentCommand::new(
        params.kb_id,
        title,
        description,
        file_name,
        file_content,
        file_type,
    )
    .execute(&app_state)
    .await?;
    Ok(document.into())
}

pub async fn get_knowledge_base_documents(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<KnowledgeBaseParams>,
    Query(query): Query<GetDocumentsQuery>,
) -> ApiResult<PaginatedResponse<AiKnowledgeBaseDocument>> {
    GetAiKnowledgeBaseByIdQuery::new(deployment_id, params.kb_id)
        .execute(&app_state)
        .await
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                "Knowledge base not found".to_string(),
            )
        })?;

    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);

    let mut documents = GetKnowledgeBaseDocumentsQuery::new(params.kb_id, limit + 1, offset)
        .execute(&app_state)
        .await
        .map_err(|e| AppError::from(e))?;

    let has_more = documents.len() > limit;
    if has_more {
        documents.pop();
    }

    Ok(PaginatedResponse {
        data: documents,
        has_more,
        limit: Some(limit as i32),
        offset: Some(offset as i32),
    }
    .into())
}

pub async fn delete_knowledge_base_document(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<DocumentParams>,
) -> ApiResult<()> {
    DeleteKnowledgeBaseDocumentCommand::new(deployment_id, params.kb_id, params.document_id)
        .execute(&app_state)
        .await?;
    Ok(().into())
}
