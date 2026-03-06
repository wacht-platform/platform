use axum::{
    Json,
    extract::{Path, Query as AxumQuery, State},
};
use serde::Deserialize;
use std::collections::HashMap;

use crate::{
    api::pagination::paginate_results,
    application::response::{ApiResult, PaginatedResponse},
    application::segments::{
        assign_segment as run_assign_segment, create_segment as run_create_segment,
        delete_segment as run_delete_segment, get_segment_data as run_get_segment_data,
        list_segments as run_list_segments, remove_segment as run_remove_segment,
        update_segment as run_update_segment, validate_segment_type,
    },
    middleware::RequireDeployment,
};
use common::state::AppState;
use models::{AnalyzedEntity, Segment};
use queries::segments::{GetSegmentDataQuery, GetSegmentsQuery};

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

fn resolve_segment_pagination(params: &SegmentQueryParams) -> (i64, i64) {
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let offset = params.offset.unwrap_or(0).max(0);
    (limit, offset)
}

fn map_user_filter(filters: Option<&SegmentDataFilters>) -> Option<queries::segments::UserFilter> {
    filters.and_then(|segment_filters| {
        segment_filters
            .user
            .as_ref()
            .map(|user| queries::segments::UserFilter {
                name: user.name.clone(),
                email: user.email.clone(),
                phone: user.phone.clone(),
            })
    })
}

fn map_organization_filter(
    filters: Option<&SegmentDataFilters>,
) -> Option<queries::segments::OrganizationFilter> {
    filters.and_then(|segment_filters| {
        segment_filters.organization.as_ref().map(|organization| {
            queries::segments::OrganizationFilter {
                name: organization.name.clone(),
            }
        })
    })
}

fn map_workspace_filter(
    filters: Option<&SegmentDataFilters>,
) -> Option<queries::segments::WorkspaceFilter> {
    filters.and_then(|segment_filters| {
        segment_filters
            .workspace
            .as_ref()
            .map(|workspace| queries::segments::WorkspaceFilter {
                name: workspace.name.clone(),
            })
    })
}

fn build_segment_data_query(
    deployment_id: i64,
    payload: GetSegmentDataRequest,
) -> GetSegmentDataQuery {
    let filters = payload.filters.as_ref();

    GetSegmentDataQuery {
        deployment_id,
        target_type: payload.target_type,
        segment_id: filters.and_then(|segment_filters| segment_filters.segment_id),
        user_filter: map_user_filter(filters),
        organization_filter: map_organization_filter(filters),
        workspace_filter: map_workspace_filter(filters),
    }
}

pub async fn get_segment_data(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(payload): Json<GetSegmentDataRequest>,
) -> ApiResult<PaginatedResponse<AnalyzedEntity>> {
    let query = build_segment_data_query(deployment_id, payload);

    let entities = run_get_segment_data(&state, query).await?;

    Ok(PaginatedResponse::from(entities).into())
}

pub async fn list_segments(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    AxumQuery(params): AxumQuery<SegmentQueryParams>,
) -> ApiResult<PaginatedResponse<Segment>> {
    let (limit, offset) = resolve_segment_pagination(&params);

    let query = GetSegmentsQuery {
        deployment_id,
        offset: Some(offset),
        limit: Some(limit + 1),
        search: params.search,
        sort_key: params.sort_key,
        sort_order: params.sort_order,
    };

    let segments = run_list_segments(&state, query).await?;
    Ok(paginate_results(segments, limit as i32, Some(offset)).into())
}

pub async fn create_segment(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(payload): Json<CreateSegmentRequest>,
) -> ApiResult<Segment> {
    validate_segment_type(&payload.r#type)?;

    let segment = run_create_segment(&state, deployment_id, payload.name, payload.r#type).await?;

    Ok(segment.into())
}

pub async fn update_segment(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<SegmentParams>,
    Json(payload): Json<UpdateSegmentRequest>,
) -> ApiResult<Segment> {
    let segment = run_update_segment(&state, params.id, deployment_id, payload.name).await?;

    Ok(segment.into())
}

pub async fn delete_segment(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<SegmentParams>,
) -> ApiResult<serde_json::Value> {
    let result = run_delete_segment(&state, params.id, deployment_id).await?;

    Ok(result.into())
}

pub async fn assign_segment(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<SegmentParams>,
    Json(payload): Json<AssignEntityRequest>,
) -> ApiResult<serde_json::Value> {
    let result = run_assign_segment(&state, params.id, deployment_id, payload.entity_id).await?;

    Ok(result.into())
}

pub async fn remove_segment(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<SegmentParams>,
    Json(payload): Json<AssignEntityRequest>,
) -> ApiResult<serde_json::Value> {
    let result = run_remove_segment(&state, params.id, deployment_id, payload.entity_id).await?;

    Ok(result.into())
}
