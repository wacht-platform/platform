use crate::{
    application::composio as composio_app, application::response::ApiResult,
    middleware::RequireDeployment,
};
use common::state::AppState;

use models::{
    ComposioAuthConfigListResponse, ComposioConfigResponse, ComposioToolkitListResponse,
    EnableComposioAppRequest, UpdateComposioConfigRequest,
};

use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;

pub async fn get_composio_config(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ComposioConfigResponse> {
    let response = composio_app::get_composio_config(&app_state, deployment_id).await?;
    Ok(response.into())
}

pub async fn update_composio_config(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(updates): Json<UpdateComposioConfigRequest>,
) -> ApiResult<ComposioConfigResponse> {
    let response =
        composio_app::update_composio_config(&app_state, deployment_id, updates).await?;
    Ok(response.into())
}

#[derive(Debug, Deserialize)]
pub struct ListToolkitsQuery {
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

pub async fn list_composio_toolkits(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ListToolkitsQuery>,
) -> ApiResult<ComposioToolkitListResponse> {
    let response = composio_app::list_toolkits(
        &app_state,
        deployment_id,
        composio_app::ListToolkitsParams {
            search: params.search,
            category: params.category,
            cursor: params.cursor,
            limit: params.limit,
        },
    )
    .await?;
    Ok(response.into())
}

pub async fn enable_composio_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<EnableComposioAppRequest>,
) -> ApiResult<ComposioConfigResponse> {
    let response = composio_app::enable_app(&app_state, deployment_id, request).await?;
    Ok(response.into())
}

pub async fn disable_composio_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
) -> ApiResult<ComposioConfigResponse> {
    let response = composio_app::disable_app(&app_state, deployment_id, &slug).await?;
    Ok(response.into())
}

pub async fn list_toolkit_auth_configs(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
) -> ApiResult<ComposioAuthConfigListResponse> {
    let response =
        composio_app::list_toolkit_auth_configs(&app_state, deployment_id, &slug).await?;
    Ok(response.into())
}
