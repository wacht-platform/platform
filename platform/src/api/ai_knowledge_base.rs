use crate::middleware::RequireDeployment;
use axum::{
    extract::{Json, Multipart, Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::api::multipart::MultipartPayload;
use crate::application::{
    ai_knowledge_base as ai_knowledge_base_use_cases,
    response::{ApiResult, PaginatedResponse},
};
use common::state::AppState;

use dto::{
    json::ai_knowledge_base::{
        CreateKnowledgeBaseRequest, GetDocumentsQuery, KnowledgeBaseResponse,
        UpdateKnowledgeBaseRequest,
    },
    query::deployment::GetKnowledgeBasesQuery,
};
use models::{AiKnowledgeBase, AiKnowledgeBaseDocument, AiKnowledgeBaseWithDetails};

#[derive(Deserialize)]
pub struct KnowledgeBaseParams {
    pub kb_id: i64,
}

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

pub async fn get_ai_knowledge_bases(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<GetKnowledgeBasesQuery>,
) -> ApiResult<KnowledgeBaseResponse> {
    let response =
        ai_knowledge_base_use_cases::get_ai_knowledge_bases(&app_state, deployment_id, query)
            .await?;
    Ok(response.into())
}

pub async fn create_ai_knowledge_base(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateKnowledgeBaseRequest>,
) -> ApiResult<AiKnowledgeBase> {
    let knowledge_base =
        ai_knowledge_base_use_cases::create_ai_knowledge_base(&app_state, deployment_id, request)
            .await?;
    Ok(knowledge_base.into())
}

pub async fn get_ai_knowledge_base_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<KnowledgeBaseParams>,
) -> ApiResult<AiKnowledgeBaseWithDetails> {
    let knowledge_base = ai_knowledge_base_use_cases::get_ai_knowledge_base_by_id(
        &app_state,
        deployment_id,
        params.kb_id,
    )
    .await?;
    Ok(knowledge_base.into())
}

pub async fn get_agent_knowledge_bases(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
) -> ApiResult<PaginatedResponse<AiKnowledgeBaseWithDetails>> {
    let knowledge_bases = ai_knowledge_base_use_cases::get_agent_knowledge_bases(
        &app_state,
        deployment_id,
        params.agent_id,
    )
    .await?;
    Ok(knowledge_bases.into())
}

pub async fn attach_knowledge_base_to_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentKnowledgeBaseParams>,
) -> ApiResult<()> {
    ai_knowledge_base_use_cases::attach_knowledge_base_to_agent(
        &app_state,
        deployment_id,
        params.agent_id,
        params.kb_id,
    )
    .await?;
    Ok(().into())
}

pub async fn detach_knowledge_base_from_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentKnowledgeBaseParams>,
) -> ApiResult<()> {
    ai_knowledge_base_use_cases::detach_knowledge_base_from_agent(
        &app_state,
        deployment_id,
        params.agent_id,
        params.kb_id,
    )
    .await?;
    Ok(().into())
}

pub async fn update_ai_knowledge_base(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<KnowledgeBaseParams>,
    Json(request): Json<UpdateKnowledgeBaseRequest>,
) -> ApiResult<AiKnowledgeBase> {
    let knowledge_base = ai_knowledge_base_use_cases::update_ai_knowledge_base(
        &app_state,
        deployment_id,
        params.kb_id,
        request,
    )
    .await?;
    Ok(knowledge_base.into())
}

pub async fn delete_ai_knowledge_base(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<KnowledgeBaseParams>,
) -> ApiResult<()> {
    ai_knowledge_base_use_cases::delete_ai_knowledge_base(&app_state, deployment_id, params.kb_id)
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

    let document = ai_knowledge_base_use_cases::upload_knowledge_base_document(
        &app_state,
        deployment_id,
        params.kb_id,
        ai_knowledge_base_use_cases::UploadKnowledgeBaseDocumentInput {
            title,
            description,
            file_content,
            file_name,
            file_type,
        },
    )
    .await?;

    Ok(document.into())
}

pub async fn get_knowledge_base_documents(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<KnowledgeBaseParams>,
    Query(query): Query<GetDocumentsQuery>,
) -> ApiResult<PaginatedResponse<AiKnowledgeBaseDocument>> {
    let documents = ai_knowledge_base_use_cases::get_knowledge_base_documents(
        &app_state,
        deployment_id,
        params.kb_id,
        query,
    )
    .await?;
    Ok(documents.into())
}

pub async fn delete_knowledge_base_document(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<DocumentParams>,
) -> ApiResult<()> {
    ai_knowledge_base_use_cases::delete_knowledge_base_document(
        &app_state,
        deployment_id,
        params.kb_id,
        params.document_id,
    )
    .await?;
    Ok(().into())
}
