use axum::extract::{Json, Path, Query, State};

use crate::application::{api_key_key as api_key_key_app, response::ApiResult};
use crate::middleware::RequireDeployment;
use common::state::AppState;
use dto::json::api_key::*;
use models::api_key::ApiKeyWithSecret;

pub async fn list_api_keys(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<ListApiKeysQuery>,
) -> ApiResult<ListApiKeysResponse> {
    let keys =
        api_key_key_app::list_api_keys(&app_state, deployment_id, app_slug, params).await?;
    Ok(keys.into())
}

pub async fn create_api_key(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Json(request): Json<CreateApiKeyRequest>,
) -> ApiResult<ApiKeyWithSecret> {
    let key =
        api_key_key_app::create_api_key(&app_state, deployment_id, app_slug, request).await?;
    Ok(key.into())
}

pub async fn revoke_api_key(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<RevokeApiKeyRequest>,
) -> ApiResult<()> {
    api_key_key_app::revoke_api_key(&app_state, deployment_id, request).await?;
    Ok(().into())
}

pub async fn revoke_api_key_for_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, key_id)): Path<(String, i64)>,
    Json(request): Json<RevokeApiKeyRequest>,
) -> ApiResult<()> {
    api_key_key_app::revoke_api_key_for_app(
        &app_state,
        deployment_id,
        app_slug,
        key_id,
        request,
    )
    .await?;

    Ok(().into())
}

pub async fn rotate_api_key(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<RotateApiKeyRequest>,
) -> ApiResult<ApiKeyWithSecret> {
    let key = api_key_key_app::rotate_api_key(&app_state, deployment_id, request).await?;
    Ok(key.into())
}

pub async fn rotate_api_key_for_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, key_id)): Path<(String, i64)>,
) -> ApiResult<ApiKeyWithSecret> {
    let key =
        api_key_key_app::rotate_api_key_for_app(&app_state, deployment_id, app_slug, key_id)
            .await?;
    Ok(key.into())
}
