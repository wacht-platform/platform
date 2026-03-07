use commands::api_key::{CreateApiKeyCommand, RevokeApiKeyCommand, RotateApiKeyCommand};
use common::db_router::ReadConsistency;
use common::state::AppState;
use dto::json::api_key::{
    CreateApiKeyRequest, ListApiKeysQuery, ListApiKeysResponse, RevokeApiKeyRequest,
    RotateApiKeyRequest,
};
use models::api_key::ApiKeyWithSecret;
use models::error::AppError;
use queries::api_key::GetApiKeysByAppQuery;

use super::api_key_shared::{
    ensure_api_key_exists_for_app, get_api_auth_app_by_slug, resolve_api_key_membership_context,
};

pub async fn list_api_keys(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
    params: ListApiKeysQuery,
) -> Result<ListApiKeysResponse, AppError> {
    let app = get_api_auth_app_by_slug(app_state, deployment_id, app_slug).await?;
    let reader = app_state.db_router.reader(ReadConsistency::Strong);

    let include_inactive = params.include_inactive.unwrap_or(false);
    let keys = GetApiKeysByAppQuery::new(app.app_slug.clone(), deployment_id)
        .with_inactive(include_inactive)
        .execute_with(reader)
        .await?;

    Ok(ListApiKeysResponse { keys })
}

pub async fn create_api_key(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
    request: CreateApiKeyRequest,
) -> Result<ApiKeyWithSecret, AppError> {
    let writer = app_state.db_router.writer();
    let app = get_api_auth_app_by_slug(app_state, deployment_id, app_slug).await?;

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

    let membership_context = resolve_api_key_membership_context(app_state, &app).await?;

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

    command
        .with_key_id(app_state.sf.next_id()? as i64)
        .execute_with(writer)
        .await
}

pub async fn revoke_api_key(
    app_state: &AppState,
    deployment_id: i64,
    request: RevokeApiKeyRequest,
) -> Result<(), AppError> {
    let writer = app_state.db_router.writer();
    let key_id = request
        .key_id
        .map(|v| v.get())
        .ok_or_else(|| AppError::BadRequest("key_id is required".to_string()))?;

    let command = RevokeApiKeyCommand {
        key_id,
        deployment_id,
        reason: request.reason,
    };
    command.execute_with(writer).await?;

    Ok(())
}

pub async fn revoke_api_key_for_app(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
    key_id: i64,
    request: RevokeApiKeyRequest,
) -> Result<(), AppError> {
    let writer = app_state.db_router.writer();
    let app = get_api_auth_app_by_slug(app_state, deployment_id, app_slug).await?;
    ensure_api_key_exists_for_app(app_state, deployment_id, &app.app_slug, key_id).await?;

    let command = RevokeApiKeyCommand {
        key_id,
        deployment_id,
        reason: request.reason,
    };
    command.execute_with(writer).await?;

    Ok(())
}

pub async fn rotate_api_key(
    app_state: &AppState,
    deployment_id: i64,
    request: RotateApiKeyRequest,
) -> Result<ApiKeyWithSecret, AppError> {
    let writer = app_state.db_router.writer();
    let command = RotateApiKeyCommand {
        key_id: request.key_id.get(),
        deployment_id,
        new_key_id: None,
    };
    command
        .with_new_key_id(app_state.sf.next_id()? as i64)
        .execute_with(writer)
        .await
}

pub async fn rotate_api_key_for_app(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
    key_id: i64,
) -> Result<ApiKeyWithSecret, AppError> {
    let writer = app_state.db_router.writer();
    let app = get_api_auth_app_by_slug(app_state, deployment_id, app_slug).await?;
    ensure_api_key_exists_for_app(app_state, deployment_id, &app.app_slug, key_id).await?;

    let command = RotateApiKeyCommand {
        key_id,
        deployment_id,
        new_key_id: None,
    };
    command
        .with_new_key_id(app_state.sf.next_id()? as i64)
        .execute_with(writer)
        .await
}
