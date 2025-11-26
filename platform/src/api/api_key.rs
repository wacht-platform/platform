use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;

use crate::application::response::ApiResult;
use crate::middleware::RequireDeployment;
use commands::{
    Command,
    api_key::{CreateApiKeyCommand, RevokeApiKeyCommand, RotateApiKeyCommand},
    api_key_app::{CreateApiKeyAppCommand, DeleteApiKeyAppCommand, UpdateApiKeyAppCommand},
};
use common::state::AppState;
use dto::json::api_key::*;
use models::api_key::{ApiKeyApp, ApiKeyWithSecret};
use queries::{
    Query as QueryTrait,
    api_key::{GetApiKeyAppByNameQuery, GetApiKeyAppsQuery, GetApiKeysByAppQuery},
};

pub async fn list_api_key_apps(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ListApiKeyAppsQuery>,
) -> ApiResult<ListApiKeyAppsResponse> {
    let include_inactive = params.include_inactive.unwrap_or(false);

    let apps = GetApiKeyAppsQuery::new(deployment_id)
        .with_inactive(include_inactive)
        .execute(&app_state)
        .await?;

    Ok(ListApiKeyAppsResponse {
        total: apps.len(),
        apps,
    }
    .into())
}

pub async fn get_api_key_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
) -> ApiResult<ApiKeyApp> {
    let app = GetApiKeyAppByNameQuery::new(deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    Ok(app.into())
}

pub async fn create_api_key_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateApiKeyAppRequest>,
) -> ApiResult<ApiKeyApp> {
    let mut command = CreateApiKeyAppCommand::new(deployment_id, request.name);

    if let Some(description) = request.description {
        command = command.with_description(description);
    }

    if let Some(rate_limits) = request.rate_limits {
        command = command.with_rate_limits(rate_limits)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    }

    let app = command.execute(&app_state).await?;
    Ok(app.into())
}

pub async fn update_api_key_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
    Json(request): Json<UpdateApiKeyAppRequest>,
) -> ApiResult<ApiKeyApp> {
    let app = GetApiKeyAppByNameQuery::new(deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    let command = UpdateApiKeyAppCommand {
        app_id: app.id,
        deployment_id,
        name: request.name,
        description: request.description,
        is_active: request.is_active,
        rate_limits: request.rate_limits,
    };

    let app = command.execute(&app_state).await?;
    Ok(app.into())
}

pub async fn delete_api_key_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
) -> ApiResult<()> {
    // First get the app by name to find its ID
    let app = GetApiKeyAppByNameQuery::new(deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    let command = DeleteApiKeyAppCommand {
        app_id: app.id,
        deployment_id,
    };
    command.execute(&app_state).await?;

    Ok(().into())
}

pub async fn list_api_keys(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
    Query(params): Query<ListApiKeysQuery>,
) -> ApiResult<ListApiKeysResponse> {
    // First get the app by name to find its ID
    let app = GetApiKeyAppByNameQuery::new(deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    let include_inactive = params.include_inactive.unwrap_or(false);

    let keys = GetApiKeysByAppQuery::new(app.id, deployment_id)
        .with_inactive(include_inactive)
        .execute(&app_state)
        .await?;

    Ok(ListApiKeysResponse { keys }.into())
}

pub async fn create_api_key(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_name): Path<String>,
    Json(request): Json<CreateApiKeyRequest>,
) -> ApiResult<ApiKeyWithSecret> {
    // First get the app by name to find its ID
    let app = GetApiKeyAppByNameQuery::new(deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    let key_prefix = request.key_prefix.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "key_prefix is required for backend API",
        )
    })?;

    let mut command = CreateApiKeyCommand::new(app.id, deployment_id, request.name, key_prefix);

    if let Some(permissions) = request.permissions {
        command = command.with_permissions(permissions);
    }

    if let Some(expires_at) = request.expires_at {
        command = command.with_expiration(expires_at);
    }

    command.metadata = request.metadata;

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
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "key_id is required"))?;

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
        key_id: request.key_id,
        deployment_id,
    };
    let new_key = command.execute(&app_state).await?;

    Ok(new_key.into())
}
