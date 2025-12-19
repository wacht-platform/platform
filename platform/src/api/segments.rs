use axum::{
    Json,
    extract::{Path, Query as AxumQuery, State},
};
use serde::Deserialize;
use std::collections::HashMap;

use crate::{
    application::{
        AppError,
        response::{ApiResult, PaginatedResponse},
    },
    middleware::RequireDeployment,
};
use commands::{
    Command,
    segments::{
        AssignSegmentCommand, CreateSegmentCommand, DeleteSegmentCommand, RemoveSegmentCommand,
        UpdateSegmentCommand,
    },
};
use common::state::AppState;
use models::{AnalyzedEntity, Segment};
use queries::{
    Query,
    segments::{GetSegmentDataQuery, GetSegmentsQuery},
};

// --- Models ---

#[derive(Deserialize)]
pub struct SegmentParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub id: i64,
}

#[derive(Deserialize)]
pub struct CreateSegmentRequest {
    pub name: String,
    pub r#type: String,
}

#[derive(Deserialize)]
pub struct UpdateSegmentRequest {
    pub name: Option<String>,
}

#[derive(Deserialize)]
pub struct SegmentQueryParams {
    pub offset: Option<i64>,
    pub limit: Option<i64>,
    pub search: Option<String>,
    pub sort_key: Option<String>,
    pub sort_order: Option<String>,
}

#[derive(Deserialize)]
pub struct AssignEntityRequest {
    #[serde(with = "common::utils::serde::i64_as_string")]
    pub entity_id: i64,
}

#[derive(Deserialize, Debug)]
pub struct UserFilter {
    pub name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct OrganizationFilter {
    pub name: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct WorkspaceFilter {
    pub name: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct SegmentDataFilters {
    pub user: Option<UserFilter>,
    pub organization: Option<OrganizationFilter>,
    pub workspace: Option<WorkspaceFilter>,
    #[serde(default)]
    #[serde(with = "common::utils::serde::i64_as_string_option")]
    pub segment_id: Option<i64>,
}

#[derive(Deserialize, Debug)]
pub struct GetSegmentDataRequest {
    pub target_type: String,
    pub filters: Option<SegmentDataFilters>,
}

pub async fn get_segment_data(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(payload): Json<GetSegmentDataRequest>,
) -> ApiResult<PaginatedResponse<AnalyzedEntity>> {
    let query = GetSegmentDataQuery {
        deployment_id,
        target_type: payload.target_type,
        segment_id: payload.filters.as_ref().and_then(|f| f.segment_id),
        user_filter: payload.filters.as_ref().and_then(|f| f.user.as_ref().map(|u| queries::segments::UserFilter {
            name: u.name.clone(),
            email: u.email.clone(),
            phone: u.phone.clone(),
        })),
        organization_filter: payload.filters.as_ref().and_then(|f| f.organization.as_ref().map(|o| queries::segments::OrganizationFilter {
            name: o.name.clone(),
        })),
        workspace_filter: payload.filters.as_ref().and_then(|f| f.workspace.as_ref().map(|w| queries::segments::WorkspaceFilter {
            name: w.name.clone(),
        })),
    };

    let entities = query.execute(&state).await?;

    Ok(PaginatedResponse::from(entities).into())
}

pub async fn list_segments(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    AxumQuery(params): AxumQuery<SegmentQueryParams>,
) -> ApiResult<PaginatedResponse<Segment>> {
    let limit = params.limit.unwrap_or(20).max(1).min(100);
    let offset = params.offset.unwrap_or(0).max(0);

    let query = GetSegmentsQuery {
        deployment_id,
        offset: Some(offset),
        limit: Some(limit + 1),
        search: params.search,
        sort_key: params.sort_key,
        sort_order: params.sort_order,
    };

    let segments = query.execute(&state).await?;

    let has_more = segments.len() > limit as usize;
    let segments = if has_more {
        segments.into_iter().take(limit as usize).collect()
    } else {
        segments
    };

    Ok(PaginatedResponse {
        data: segments,
        has_more,
        limit: Some(limit as i32),
        offset: Some(offset as i32),
    }
    .into())
}

pub async fn create_segment(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(payload): Json<CreateSegmentRequest>,
) -> ApiResult<Segment> {
    if payload.r#type != "organization" && payload.r#type != "workspace" && payload.r#type != "user"
    {
        return Err(AppError::BadRequest(
            "Invalid segment type. Must be 'organization', 'workspace', or 'user'".into(),
        )
        .into());
    }

    let command = CreateSegmentCommand {
        deployment_id,
        name: payload.name,
        r#type: payload.r#type,
    };

    let segment = command.execute(&state).await?;

    Ok(segment.into())
}

pub async fn update_segment(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<SegmentParams>,
    Json(payload): Json<UpdateSegmentRequest>,
) -> ApiResult<Segment> {
    let command = UpdateSegmentCommand {
        id: params.id,
        deployment_id,
        name: payload.name,
    };

    let segment = command.execute(&state).await?;

    Ok(segment.into())
}

pub async fn delete_segment(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<SegmentParams>,
) -> ApiResult<serde_json::Value> {
    let command = DeleteSegmentCommand {
        id: params.id,
        deployment_id,
    };

    let result = command.execute(&state).await?;

    Ok(result.into())
}

pub async fn assign_segment(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<SegmentParams>,
    Json(payload): Json<AssignEntityRequest>,
) -> ApiResult<serde_json::Value> {
    let command = AssignSegmentCommand {
        segment_id: params.id,
        deployment_id,
        entity_id: payload.entity_id,
    };

    let result = command.execute(&state).await?;

    Ok(result.into())
}

pub async fn remove_segment(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<SegmentParams>,
    Json(payload): Json<AssignEntityRequest>,
) -> ApiResult<serde_json::Value> {
    let command = RemoveSegmentCommand {
        segment_id: params.id,
        deployment_id,
        entity_id: payload.entity_id,
    };

    let result = command.execute(&state).await?;

    Ok(result.into())
}

