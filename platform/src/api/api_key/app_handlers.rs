use axum::extract::{Json, Path, Query, State};

use crate::application::{api_key_app as api_key_app_app, response::ApiResult};
use crate::middleware::{AppSlugParams, RequireDeployment};
use common::state::AppState;
use dto::json::api_key::*;
use models::api_key::ApiAuthApp;

pub async fn list_api_auth_apps(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ListApiAuthAppsQuery>,
) -> ApiResult<ListApiAuthAppsResponse> {
    let apps = api_key_app_app::list_api_auth_apps(&app_state, deployment_id, params).await?;
    Ok(apps.into())
}

pub async fn get_api_auth_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(AppSlugParams { app_slug, .. }): Path<AppSlugParams>,
) -> ApiResult<ApiAuthApp> {
    let app = api_key_app_app::get_api_auth_app(&app_state, deployment_id, app_slug).await?;
    Ok(app.into())
}

pub async fn create_api_auth_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateApiAuthAppRequest>,
) -> ApiResult<ApiAuthApp> {
    let app = api_key_app_app::create_api_auth_app(&app_state, deployment_id, request).await?;
    Ok(app.into())
}

pub async fn update_api_auth_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(AppSlugParams { app_slug, .. }): Path<AppSlugParams>,
    Json(request): Json<UpdateApiAuthAppRequest>,
) -> ApiResult<ApiAuthApp> {
    let app =
        api_key_app_app::update_api_auth_app(&app_state, deployment_id, app_slug, request).await?;
    Ok(app.into())
}

pub async fn delete_api_auth_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(AppSlugParams { app_slug, .. }): Path<AppSlugParams>,
) -> ApiResult<()> {
    api_key_app_app::delete_api_auth_app(&app_state, deployment_id, app_slug).await?;
    Ok(().into())
}
