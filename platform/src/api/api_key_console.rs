use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;

use crate::application::{HttpState, response::ApiResult};
use crate::middleware::RequireDeployment;
use shared::{
    commands::{
        Command,
        api_key::{CreateApiKeyCommand, RevokeApiKeyCommand, RotateApiKeyCommand},
        api_key_app::{CreateApiKeyAppCommand, DeleteApiKeyAppCommand, UpdateApiKeyAppCommand},
    },
    dto::json::api_key_requests::*,
    models::api_key::{ApiKeyApp, ApiKeyWithSecret},
    queries::{
        Query as QueryTrait,
        api_key::{GetApiKeyAppsQuery, GetApiKeyByIdQuery, GetApiKeysByAppQuery},
    },
};

pub async fn list_api_key_apps(
    State(app_state): State<HttpState>,
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

pub async fn create_api_key_app(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateApiKeyAppRequest>,
) -> ApiResult<ApiKeyApp> {
    let mut command = CreateApiKeyAppCommand::new(deployment_id, request.name);

    if let Some(description) = request.description {
        command = command.with_description(description);
    }

    if let Some(per_minute) = request.rate_limit_per_minute {
        if let Some(per_hour) = request.rate_limit_per_hour {
            command = command.with_rate_limits(per_minute, per_hour);
        }
    }

    let app = command.execute(&app_state).await?;
    Ok(app.into())
}

pub async fn update_api_key_app(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_id): Path<i64>,
    Json(request): Json<UpdateApiKeyAppRequest>,
) -> ApiResult<ApiKeyApp> {
    let command = UpdateApiKeyAppCommand {
        app_id,
        deployment_id,
        name: request.name,
        description: request.description,
        is_active: request.is_active,
        rate_limit_per_minute: request.rate_limit_per_minute,
        rate_limit_per_hour: request.rate_limit_per_hour,
    };

    let app = command.execute(&app_state).await?;
    Ok(app.into())
}

pub async fn delete_api_key_app(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_id): Path<i64>,
) -> ApiResult<()> {
    let command = DeleteApiKeyAppCommand {
        app_id,
        deployment_id,
    };
    command.execute(&app_state).await?;

    Ok(().into())
}

pub async fn list_api_keys(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_id): Path<i64>,
    Query(params): Query<ListApiKeysQuery>,
) -> ApiResult<ListApiKeysResponse> {
    let include_inactive = params.include_inactive.unwrap_or(false);

    let keys = GetApiKeysByAppQuery::new(app_id, deployment_id)
        .with_inactive(include_inactive)
        .execute(&app_state)
        .await?;

    Ok(ListApiKeysResponse {
        total: keys.len(),
        keys,
    }
    .into())
}

pub async fn create_api_key(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_id): Path<i64>,
    Json(request): Json<CreateApiKeyRequest>,
) -> ApiResult<ApiKeyWithSecret> {
    let mut command =
        CreateApiKeyCommand::new(app_id, deployment_id, request.name, request.key_prefix);

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
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_id, key_id)): Path<(i64, i64)>,
    Json(request): Json<RevokeApiKeyRequest>,
) -> ApiResult<()> {
    let key = GetApiKeyByIdQuery {
        key_id,
        deployment_id,
    }
    .execute(&app_state)
    .await?
    .ok_or_else(|| (StatusCode::NOT_FOUND, "API key not found"))?;

    if key.app_id != app_id {
        return Err((StatusCode::FORBIDDEN, "API key does not belong to this app").into());
    }

    let command = RevokeApiKeyCommand {
        key_id,
        deployment_id,
        reason: request.reason,
    };
    command.execute(&app_state).await?;

    Ok(().into())
}

pub async fn rotate_api_key(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_id, key_id)): Path<(i64, i64)>,
) -> ApiResult<ApiKeyWithSecret> {
    let key = GetApiKeyByIdQuery {
        key_id,
        deployment_id,
    }
    .execute(&app_state)
    .await?
    .ok_or_else(|| (StatusCode::NOT_FOUND, "API key not found"))?;

    if key.app_id != app_id {
        return Err((StatusCode::FORBIDDEN, "API key does not belong to this app").into());
    }

    let command = RotateApiKeyCommand {
        key_id,
        deployment_id,
    };
    let new_key = command.execute(&app_state).await?;

    Ok(new_key.into())
}
