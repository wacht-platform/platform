use commands::oauth::RevokeOAuthClientGrantCommand;
use common::db_router::ReadConsistency;
use common::state::AppState;
use dto::json::api_key::{ListOAuthGrantsResponse, OAuthGrantResponse};
use models::error::AppError;
use queries::{
    oauth::{ListOAuthGrantsByClientQuery, OAuthClientGrantData},
};

use super::oauth_shared::{get_oauth_app_by_slug, get_oauth_client_by_id};

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

pub async fn list_oauth_grants(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
    oauth_client_id: i64,
) -> Result<ListOAuthGrantsResponse, AppError> {
    let oauth_app = get_oauth_app_by_slug(app_state, deployment_id, oauth_app_slug).await?;
    get_oauth_client_by_id(app_state, deployment_id, oauth_app.id, oauth_client_id).await?;
    let reader = app_state.db_router.reader(ReadConsistency::Strong);

    let grants = ListOAuthGrantsByClientQuery::new(deployment_id, oauth_client_id)
        .execute_with(reader)
        .await?
        .into_iter()
        .map(map_oauth_grant_response)
        .collect();

    Ok(ListOAuthGrantsResponse { grants })
}

pub async fn revoke_oauth_grant(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
    oauth_client_id: i64,
    grant_id: i64,
) -> Result<(), AppError> {
    let writer = app_state.db_router.writer();
    let oauth_app = get_oauth_app_by_slug(app_state, deployment_id, oauth_app_slug).await?;
    get_oauth_client_by_id(app_state, deployment_id, oauth_app.id, oauth_client_id).await?;

    RevokeOAuthClientGrantCommand {
        deployment_id,
        oauth_client_id,
        grant_id,
    }
    .execute_with(writer)
    .await?;

    Ok(())
}
