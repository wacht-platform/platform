use axum::extract::{Path, State};

use crate::application::response::ApiResult;
use crate::middleware::RequireDeployment;
use commands::{Command, oauth::RevokeOAuthClientGrantCommand};
use common::state::AppState;
use dto::json::api_key::{ListOAuthGrantsResponse, OAuthGrantResponse};
use queries::{
    Query as QueryTrait,
    oauth::{ListOAuthGrantsByClientQuery, OAuthClientGrantData},
};

use super::helpers::{get_oauth_app_by_slug, get_oauth_client_by_id};
use super::types::{OAuthClientPathParams, OAuthGrantPathParams};

fn map_oauth_grant_response(g: OAuthClientGrantData) -> OAuthGrantResponse {
    let scopes = g.scopes_vec();

    OAuthGrantResponse {
        id: g.id,
        api_auth_app_slug: g.api_auth_app_slug,
        oauth_client_id: g.oauth_client_id,
        resource: g.resource,
        scopes,
        status: g.status,
        granted_at: g.granted_at,
        expires_at: g.expires_at,
        revoked_at: g.revoked_at,
        granted_by_user_id: g.granted_by_user_id,
        created_at: g.created_at,
        updated_at: g.updated_at,
    }
}

pub(crate) async fn list_oauth_grants(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthClientPathParams>,
) -> ApiResult<ListOAuthGrantsResponse> {
    let oauth_app = get_oauth_app_by_slug(&app_state, deployment_id, params.oauth_app_slug).await?;
    get_oauth_client_by_id(
        &app_state,
        deployment_id,
        oauth_app.id,
        params.oauth_client_id,
    )
    .await?;

    let grants = ListOAuthGrantsByClientQuery::new(deployment_id, params.oauth_client_id)
        .execute(&app_state)
        .await?
        .into_iter()
        .map(map_oauth_grant_response)
        .collect();

    Ok(ListOAuthGrantsResponse { grants }.into())
}

pub(crate) async fn revoke_oauth_grant(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthGrantPathParams>,
) -> ApiResult<()> {
    let oauth_app = get_oauth_app_by_slug(&app_state, deployment_id, params.oauth_app_slug).await?;
    get_oauth_client_by_id(
        &app_state,
        deployment_id,
        oauth_app.id,
        params.oauth_client_id,
    )
    .await?;

    RevokeOAuthClientGrantCommand {
        deployment_id,
        oauth_client_id: params.oauth_client_id,
        grant_id: params.grant_id,
    }
    .execute(&app_state)
    .await?;

    Ok(().into())
}
