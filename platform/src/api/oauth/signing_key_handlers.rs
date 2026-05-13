//! Console-side admin endpoints for OIDC per-app signing keys. Handlers are
//! thin — they resolve the OAuth app from its slug + the caller's deployment
//! and forward to the runtime layer.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use common::state::AppState;
use dto::json::oauth_runtime::{
    OAuthAppSigningKeyRotatedResponse, OAuthAppSigningKeySummary,
    OAuthAppSigningKeysListResponse,
};
use queries::oauth_runtime::OAuthAppPublishableKey;

use crate::application::oauth_runtime as oauth_runtime_app;
use crate::application::oauth_shared::get_oauth_app_by_slug;
use crate::application::response::{ApiErrorResponse, ApiResult};
use crate::middleware::RequireDeployment;

use super::types::{OAuthAppPathParams, OAuthSigningKeyPathParams};

pub(crate) async fn list_signing_keys(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthAppPathParams>,
) -> ApiResult<OAuthAppSigningKeysListResponse> {
    let oauth_app = get_oauth_app_by_slug(&app_state, deployment_id, params.oauth_app_slug)
        .await
        .map_err(ApiErrorResponse::from)?;
    let keys = oauth_runtime_app::list_app_signing_keys(&app_state, oauth_app.id).await?;
    Ok(OAuthAppSigningKeysListResponse {
        keys: keys.into_iter().map(to_summary).collect(),
    }
    .into())
}

pub(crate) async fn rotate_signing_key(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthAppPathParams>,
) -> ApiResult<OAuthAppSigningKeyRotatedResponse> {
    let oauth_app = get_oauth_app_by_slug(&app_state, deployment_id, params.oauth_app_slug)
        .await
        .map_err(ApiErrorResponse::from)?;
    let new_key = oauth_runtime_app::rotate_app_signing_key(&app_state, oauth_app.id).await?;
    Ok(OAuthAppSigningKeyRotatedResponse {
        new: to_summary(new_key),
    }
    .into())
}

pub(crate) async fn compromise_signing_key(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthSigningKeyPathParams>,
) -> ApiResult<()> {
    let oauth_app = get_oauth_app_by_slug(&app_state, deployment_id, params.oauth_app_slug)
        .await
        .map_err(ApiErrorResponse::from)?;
    oauth_runtime_app::compromise_app_signing_key(&app_state, oauth_app.id, params.kid).await?;
    Ok((StatusCode::NO_CONTENT, ()).into())
}

fn to_summary(key: OAuthAppPublishableKey) -> OAuthAppSigningKeySummary {
    // private_key_pem deliberately dropped — admin UI shows the public half
    // for verification by external tooling; the private half stays in the DB.
    OAuthAppSigningKeySummary {
        kid: key.kid,
        algorithm: key.algorithm,
        status: key.status,
        public_key_pem: key.public_key_pem,
    }
}
