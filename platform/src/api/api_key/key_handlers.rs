use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;

use super::helpers::{
    ensure_api_key_exists_for_app, get_api_auth_app_by_slug, resolve_api_key_membership_context,
};
use crate::application::response::ApiResult;
use crate::middleware::RequireDeployment;
use commands::{
    Command,
    api_key::{CreateApiKeyCommand, RevokeApiKeyCommand, RotateApiKeyCommand},
};
use common::state::AppState;
use dto::json::api_key::*;
use models::api_key::ApiKeyWithSecret;
use queries::{Query as QueryTrait, api_key::GetApiKeysByAppQuery};

pub async fn list_api_keys(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<ListApiKeysQuery>,
) -> ApiResult<ListApiKeysResponse> {
    let app = get_api_auth_app_by_slug(&app_state, deployment_id, app_slug).await?;

    let include_inactive = params.include_inactive.unwrap_or(false);

    let keys = GetApiKeysByAppQuery::new(app.app_slug.clone(), deployment_id)
        .with_inactive(include_inactive)
        .execute(&app_state)
        .await?;

    Ok(ListApiKeysResponse { keys }.into())
}

pub async fn create_api_key(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Json(request): Json<CreateApiKeyRequest>,
) -> ApiResult<ApiKeyWithSecret> {
    let app = get_api_auth_app_by_slug(&app_state, deployment_id, app_slug).await?;

    let mut command = CreateApiKeyCommand::new(
        app.app_slug.clone(),
        deployment_id,
        request.name,
        app.key_prefix.clone(),
    );
    command.owner_user_id = app.user_id;

    if let Some(permissions) = request.permissions {
        command = command.with_permissions(permissions);
    }

    let membership_context = resolve_api_key_membership_context(&app_state, &app).await?;

    if let Some(expires_at) = request.expires_at {
        command = command.with_expiration(expires_at);
    }

    command.metadata = request.metadata;

    command = command
        .with_rate_limit_scheme_slug(app.rate_limit_scheme_slug.clone())
        .with_membership_context(
            membership_context.organization_id,
            membership_context.workspace_id,
            membership_context.organization_membership_id,
            membership_context.workspace_membership_id,
            membership_context.org_role_permissions,
            membership_context.workspace_role_permissions,
        );

    let key_with_secret = command.execute(&app_state).await?;

    Ok(key_with_secret.into())
}

pub async fn revoke_api_key(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<RevokeApiKeyRequest>,
) -> ApiResult<()> {
    let key_id = request
        .key_id
        .map(|v| v.get())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "key_id is required"))?;

    let command = RevokeApiKeyCommand {
        key_id,
        deployment_id,
        reason: request.reason,
    };
    command.execute(&app_state).await?;

    Ok(().into())
}

pub async fn revoke_api_key_for_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, key_id)): Path<(String, i64)>,
    Json(request): Json<RevokeApiKeyRequest>,
) -> ApiResult<()> {
    let app = get_api_auth_app_by_slug(&app_state, deployment_id, app_slug).await?;
    ensure_api_key_exists_for_app(&app_state, deployment_id, &app.app_slug, key_id).await?;

    let command = RevokeApiKeyCommand {
        key_id,
        deployment_id,
        reason: request.reason,
    };
    command.execute(&app_state).await?;

    Ok(().into())
}

pub async fn rotate_api_key(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<RotateApiKeyRequest>,
) -> ApiResult<ApiKeyWithSecret> {
    let command = RotateApiKeyCommand {
        key_id: request.key_id.get(),
        deployment_id,
    };
    let new_key = command.execute(&app_state).await?;

    Ok(new_key.into())
}

pub async fn rotate_api_key_for_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, key_id)): Path<(String, i64)>,
) -> ApiResult<ApiKeyWithSecret> {
    let app = get_api_auth_app_by_slug(&app_state, deployment_id, app_slug).await?;
    ensure_api_key_exists_for_app(&app_state, deployment_id, &app.app_slug, key_id).await?;

    let command = RotateApiKeyCommand {
        key_id,
        deployment_id,
    };
    let new_key = command.execute(&app_state).await?;

    Ok(new_key.into())
}
