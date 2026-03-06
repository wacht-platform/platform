use axum::extract::{Path, State};

use crate::application::{oauth_grant as oauth_grant_use_cases, response::ApiResult};
use crate::middleware::RequireDeployment;
use common::state::AppState;
use dto::json::api_key::ListOAuthGrantsResponse;

use super::types::{OAuthClientPathParams, OAuthGrantPathParams};

pub(crate) async fn list_oauth_grants(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthClientPathParams>,
) -> ApiResult<ListOAuthGrantsResponse> {
    let grants = oauth_grant_use_cases::list_oauth_grants(
        &app_state,
        deployment_id,
        params.oauth_app_slug,
        params.oauth_client_id,
    )
    .await?;

    Ok(grants.into())
}

pub(crate) async fn revoke_oauth_grant(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthGrantPathParams>,
) -> ApiResult<()> {
    oauth_grant_use_cases::revoke_oauth_grant(
        &app_state,
        deployment_id,
        params.oauth_app_slug,
        params.oauth_client_id,
        params.grant_id,
    )
    .await?;

    Ok(().into())
}
