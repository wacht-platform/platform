use axum::http::StatusCode;
use common::state::AppState;
use queries::{
    Query as QueryTrait,
    oauth::{GetOAuthAppBySlugQuery, GetOAuthClientByIdQuery, OAuthAppData, OAuthClientData},
};

use crate::application::response::ApiErrorResponse;

pub(crate) async fn get_oauth_app_by_slug(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
) -> Result<OAuthAppData, ApiErrorResponse> {
    GetOAuthAppBySlugQuery::new(deployment_id, oauth_app_slug)
        .execute(app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found").into())
}

pub(crate) async fn get_oauth_client_by_id(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_id: i64,
    oauth_client_id: i64,
) -> Result<OAuthClientData, ApiErrorResponse> {
    GetOAuthClientByIdQuery::new(deployment_id, oauth_app_id, oauth_client_id)
        .execute(app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found").into())
}
