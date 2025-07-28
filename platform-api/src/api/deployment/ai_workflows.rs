use axum::extract::{Json, Path, Query, State};

use crate::{
    application::{
        HttpState,
        response::{ApiResult, PaginatedResponse},
    },
    core::{
        commands::{
            Command, CreateAiWorkflowCommand, DeleteAiWorkflowCommand, UpdateAiWorkflowCommand,
        },
        dto::{
            json::deployment::{CreateWorkflowRequest, UpdateWorkflowRequest},
            query::deployment::GetWorkflowsQuery,
        },
        models::{AiWorkflow, AiWorkflowWithDetails},
        queries::{GetAiWorkflowByIdQuery, GetAiWorkflowsQuery, Query as QueryTrait},
    },
};

pub async fn get_ai_workflows(
    State(app_state): State<HttpState>,
    Path(deployment_id): Path<i64>,
    Query(query): Query<GetWorkflowsQuery>,
) -> ApiResult<PaginatedResponse<AiWorkflowWithDetails>> {
    let limit = query.limit.unwrap_or(50) as u32;

    let workflows = GetAiWorkflowsQuery::new(deployment_id)
        .with_limit(Some(limit + 1))
        .with_offset(query.offset.map(|o| o as u32))
        .with_search(query.search)
        .execute(&app_state)
        .await?;

    let has_more = workflows.len() > limit as usize;
    let workflows = if has_more {
        workflows[..limit as usize].to_vec()
    } else {
        workflows
    };

    Ok(PaginatedResponse {
        data: workflows,
        has_more,
    }
    .into())
}

pub async fn create_ai_workflow(
    State(app_state): State<HttpState>,
    Path(deployment_id): Path<i64>,
    Json(request): Json<CreateWorkflowRequest>,
) -> ApiResult<AiWorkflow> {
    CreateAiWorkflowCommand::new(
        deployment_id,
        request.name,
        request.description,
        request.configuration.unwrap_or_default(),
        request.workflow_definition.unwrap_or_default(),
    )
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}

pub async fn get_ai_workflow_by_id(
    State(app_state): State<HttpState>,
    Path((deployment_id, workflow_id)): Path<(i64, i64)>,
) -> ApiResult<AiWorkflowWithDetails> {
    GetAiWorkflowByIdQuery::new(deployment_id, workflow_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_ai_workflow(
    State(app_state): State<HttpState>,
    Path((deployment_id, workflow_id)): Path<(i64, i64)>,
    Json(request): Json<UpdateWorkflowRequest>,
) -> ApiResult<AiWorkflow> {
    let mut command = UpdateAiWorkflowCommand::new(deployment_id, workflow_id);

    if let Some(name) = request.name {
        command = command.with_name(name);
    }
    if let Some(description) = request.description {
        command = command.with_description(Some(description));
    }
    if let Some(configuration) = request.configuration {
        command = command.with_configuration(configuration);
    }
    if let Some(workflow_definition) = request.workflow_definition {
        command = command.with_workflow_definition(workflow_definition);
    }

    command
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn delete_ai_workflow(
    State(app_state): State<HttpState>,
    Path((deployment_id, workflow_id)): Path<(i64, i64)>,
) -> ApiResult<()> {
    DeleteAiWorkflowCommand::new(deployment_id, workflow_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}
