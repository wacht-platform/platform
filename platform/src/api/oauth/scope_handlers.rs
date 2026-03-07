use axum::Json;
use axum::extract::{Path, State};

use crate::application::{oauth_scope as oauth_scope_app, response::ApiResult};
use crate::middleware::RequireDeployment;
use common::state::AppState;
use dto::json::api_key::{OAuthAppResponse, SetOAuthScopeMappingRequest, UpdateOAuthScopeRequest};

use super::types::OAuthScopePathParams;

pub(crate) async fn update_oauth_scope(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthScopePathParams>,
    Json(request): Json<UpdateOAuthScopeRequest>,
) -> ApiResult<OAuthAppResponse> {
    let app = oauth_scope_app::update_oauth_scope(
        &app_state,
        deployment_id,
        params.oauth_app_slug,
        params.scope,
        request,
    )
    .await?;
    Ok(app.into())
}

pub(crate) async fn archive_oauth_scope(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthScopePathParams>,
) -> ApiResult<OAuthAppResponse> {
    let app = oauth_scope_app::archive_oauth_scope(
        &app_state,
        deployment_id,
        params.oauth_app_slug,
        params.scope,
    )
    .await?;
    Ok(app.into())
}

pub(crate) async fn unarchive_oauth_scope(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthScopePathParams>,
) -> ApiResult<OAuthAppResponse> {
    let app = oauth_scope_app::unarchive_oauth_scope(
        &app_state,
        deployment_id,
        params.oauth_app_slug,
        params.scope,
    )
    .await?;
    Ok(app.into())
}

pub(crate) async fn set_oauth_scope_mapping(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthScopePathParams>,
    Json(request): Json<SetOAuthScopeMappingRequest>,
) -> ApiResult<OAuthAppResponse> {
    let app = oauth_scope_app::set_oauth_scope_mapping(
        &app_state,
        deployment_id,
        params.oauth_app_slug,
        params.scope,
        request,
    )
    .await?;

    Ok(app.into())
}
