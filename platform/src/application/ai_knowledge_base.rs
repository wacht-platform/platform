use commands::{
    AttachKnowledgeBaseToAgentCommand, CreateAiKnowledgeBaseCommand, DeleteAiKnowledgeBaseCommand,
    DeleteKnowledgeBaseDocumentCommand, DetachKnowledgeBaseFromAgentCommand,
    UpdateAiKnowledgeBaseCommand, UploadKnowledgeBaseDocumentCommand,
};
use common::ReadConsistency;
use common::deps;
use common::error::AppError;
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
};

use crate::{
    api::pagination::paginate_results,
    application::{
        AppState,
        response::{ApiErrorResponse, PaginatedResponse},
    },
};

pub struct UploadKnowledgeBaseDocumentInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub file_content: Vec<u8>,
    pub file_name: String,
    pub file_type: Option<String>,
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

fn knowledge_base_not_found_error() -> ApiErrorResponse {
    (
        axum::http::StatusCode::NOT_FOUND,
        "Knowledge base not found".to_string(),
    )
        .into()
}

pub async fn get_ai_knowledge_bases(
    app_state: &AppState,
    deployment_id: i64,
    query: GetKnowledgeBasesQuery,
) -> Result<KnowledgeBaseResponse, AppError> {
    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);

    let mut query_builder = GetKnowledgeBasesQueryCore::new(deployment_id, limit + 1, offset);
    if let Some(search) = query.search {
        query_builder = query_builder.with_search(search);
    }

    let knowledge_bases = query_builder
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;
    let paginated = paginate_results(knowledge_bases, limit as i32, Some(offset as i64));

    Ok(KnowledgeBaseResponse {
        data: paginated.data,
        has_more: paginated.has_more,
    })
}

pub async fn create_ai_knowledge_base(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateKnowledgeBaseRequest,
) -> Result<AiKnowledgeBase, AppError> {
    let configuration = request.configuration.unwrap_or(serde_json::json!({}));
    let create_command = CreateAiKnowledgeBaseCommand::new(
        deployment_id,
        request.name,
        request.description,
        configuration,
    )
    .with_knowledge_base_id(app_state.sf.next_id()? as i64);
    create_command
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn get_ai_knowledge_base_by_id(
    app_state: &AppState,
    deployment_id: i64,
    kb_id: i64,
) -> Result<AiKnowledgeBaseWithDetails, AppError> {
    GetAiKnowledgeBaseByIdQuery::new(deployment_id, kb_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn get_agent_knowledge_bases(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
) -> Result<PaginatedResponse<AiKnowledgeBaseWithDetails>, AppError> {
    let knowledge_bases = GetAgentKnowledgeBasesQuery::new(deployment_id, agent_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;
    Ok(PaginatedResponse::from(knowledge_bases))
}

pub async fn attach_knowledge_base_to_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    kb_id: i64,
) -> Result<(), AppError> {
    AttachKnowledgeBaseToAgentCommand::new(deployment_id, agent_id, kb_id)
        .execute_with_db(app_state.db_router.writer())
        .await?;
    Ok(())
}

pub async fn detach_knowledge_base_from_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    kb_id: i64,
) -> Result<(), AppError> {
    DetachKnowledgeBaseFromAgentCommand::new(deployment_id, agent_id, kb_id)
        .execute_with_db(app_state.db_router.writer())
        .await?;
    Ok(())
}

pub async fn update_ai_knowledge_base(
    app_state: &AppState,
    deployment_id: i64,
    kb_id: i64,
    request: UpdateKnowledgeBaseRequest,
) -> Result<AiKnowledgeBase, AppError> {
    let mut command = UpdateAiKnowledgeBaseCommand::new(deployment_id, kb_id);

    if let Some(name) = request.name {
        command = command.with_name(name);
    }
    if let Some(description) = request.description {
        command = command.with_description(Some(description));
    }
    if let Some(configuration) = request.configuration {
        command = command.with_configuration(configuration);
    }

    command.execute_with_db(app_state.db_router.writer()).await
}

pub async fn delete_ai_knowledge_base(
    app_state: &AppState,
    deployment_id: i64,
    kb_id: i64,
) -> Result<(), AppError> {
    let deps = deps::from_app(app_state).db();
    DeleteAiKnowledgeBaseCommand::new(deployment_id, kb_id)
        .execute_with_deps(&deps)
        .await?;
    Ok(())
}

pub async fn upload_knowledge_base_document(
    app_state: &AppState,
    deployment_id: i64,
    kb_id: i64,
    input: UploadKnowledgeBaseDocumentInput,
) -> Result<AiKnowledgeBaseDocument, ApiErrorResponse> {
    let file_name = sanitize_upload_filename(&input.file_name).ok_or((
        axum::http::StatusCode::BAD_REQUEST,
        "Invalid filename".to_string(),
    ))?;
    let file_type = input
        .file_type
        .unwrap_or("application/octet-stream".to_string());

    let title = input.title.unwrap_or_else(|| {
        file_name
            .split('.')
            .next()
            .unwrap_or(&file_name)
            .to_string()
    });

    if input.file_content.is_empty() {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "File content is empty".to_string(),
        )
            .into());
    }

    GetAiKnowledgeBaseByIdQuery::new(deployment_id, kb_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await
        .map_err(|_| knowledge_base_not_found_error())?;

    let deps = deps::from_app(app_state).db().nats();
    let document = UploadKnowledgeBaseDocumentCommand::new(
        kb_id,
        title,
        input.description,
        file_name,
        input.file_content,
        file_type,
    )
    .with_document_id(
        app_state
            .sf
            .next_id()
            .map_err(|e| AppError::Internal(e.to_string()))? as i64,
    )
    .execute_with_deps(&deps)
    .await?;

    Ok(document)
}

pub async fn get_knowledge_base_documents(
    app_state: &AppState,
    deployment_id: i64,
    kb_id: i64,
    query: GetDocumentsQuery,
) -> Result<PaginatedResponse<AiKnowledgeBaseDocument>, ApiErrorResponse> {
    GetAiKnowledgeBaseByIdQuery::new(deployment_id, kb_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await
        .map_err(|_| knowledge_base_not_found_error())?;

    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);
    let documents = GetKnowledgeBaseDocumentsQuery::new(kb_id, limit + 1, offset)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    Ok(paginate_results(
        documents,
        limit as i32,
        Some(offset as i64),
    ))
}

pub async fn delete_knowledge_base_document(
    app_state: &AppState,
    deployment_id: i64,
    kb_id: i64,
    document_id: i64,
) -> Result<(), AppError> {
    let deps = deps::from_app(app_state).db();
    DeleteKnowledgeBaseDocumentCommand::new(deployment_id, kb_id, document_id)
        .execute_with_deps(&deps)
        .await?;
    Ok(())
}
