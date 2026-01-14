use crate::middleware::RequireDeployment;
use axum::extract::{Json, Path, Query, State};
use serde::Deserialize;

use crate::application::response::{ApiResult, PaginatedResponse};
use common::state::AppState;

use commands::{
    Command, CreateAiWorkflowCommand, DeleteAiWorkflowCommand, UpdateAiWorkflowCommand,
};
use dto::{
    json::deployment::{CreateWorkflowRequest, UpdateWorkflowRequest},
    query::deployment::GetWorkflowsQuery,
};
use models::{AiWorkflow, AiWorkflowWithDetails};
use queries::{GetAiWorkflowByIdQuery, GetAiWorkflowsQuery, Query as QueryTrait};

#[derive(Deserialize)]
pub struct WorkflowParams {
    pub workflow_id: i64,
}

pub async fn get_ai_workflows(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
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
        limit: Some(limit as i32),
        offset: query.offset.map(|o| o as i32),
    }
    .into())
}

pub async fn create_ai_workflow(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
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
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkflowParams>,
) -> ApiResult<AiWorkflowWithDetails> {
    GetAiWorkflowByIdQuery::new(deployment_id, params.workflow_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_ai_workflow(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkflowParams>,
    Json(request): Json<UpdateWorkflowRequest>,
) -> ApiResult<AiWorkflow> {
    let mut command = UpdateAiWorkflowCommand::new(deployment_id, params.workflow_id);

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
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkflowParams>,
) -> ApiResult<()> {
    DeleteAiWorkflowCommand::new(deployment_id, params.workflow_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}
