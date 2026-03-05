use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;

use crate::application::response::ApiResult;
use crate::middleware::RequireDeployment;
use commands::{
    Command,
    api_key::{CreateApiKeyCommand, RevokeApiKeyCommand, RotateApiKeyCommand},
};
use common::state::AppState;
use dto::json::api_key::*;
use models::api_key::ApiKeyWithSecret;
use queries::{
    Query as QueryTrait,
    api_key::{
        GetApiAuthAppBySlugQuery, GetApiKeysByAppQuery,
        GetOrganizationMembershipIdByUserAndOrganizationQuery,
        GetOrganizationMembershipPermissionsQuery, GetWorkspaceMembershipIdByUserAndWorkspaceQuery,
        GetWorkspaceMembershipPermissionsQuery,
    },
};

pub async fn list_api_keys(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<ListApiKeysQuery>,
) -> ApiResult<ListApiKeysResponse> {
    // First get the app by name to find its ID
    let app = GetApiAuthAppBySlugQuery::new(deployment_id, app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

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
    // First get the app by name to find its ID
    let app = GetApiAuthAppBySlugQuery::new(deployment_id, app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

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

    let mut org_membership_id: Option<i64> = None;
    let mut workspace_membership_id: Option<i64> = None;

    if let (Some(user_id), Some(organization_id)) = (app.user_id, app.organization_id) {
        org_membership_id =
            GetOrganizationMembershipIdByUserAndOrganizationQuery::new(user_id, organization_id)
                .execute(&app_state)
                .await?;
        if org_membership_id.is_none() {
            return Err((StatusCode::BAD_REQUEST, "user is not a member of the org").into());
        }
    }

    if let (Some(user_id), Some(workspace_id)) = (app.user_id, app.workspace_id) {
        workspace_membership_id =
            GetWorkspaceMembershipIdByUserAndWorkspaceQuery::new(user_id, workspace_id)
                .execute(&app_state)
                .await?;
        if workspace_membership_id.is_none() {
            return Err((StatusCode::BAD_REQUEST, "user is not a member of the org").into());
        }
    }

    let mut organization_id: Option<i64> = None;
    let mut workspace_id: Option<i64> = None;
    let mut org_role_permissions: Vec<String> = vec![];
    let mut workspace_role_permissions: Vec<String> = vec![];

    if let Some(org_membership_id) = org_membership_id {
        let org_perm = GetOrganizationMembershipPermissionsQuery::new(org_membership_id)
            .execute(&app_state)
            .await?
            .ok_or_else(|| (StatusCode::NOT_FOUND, "Organization membership not found"))?;
        organization_id = Some(org_perm.organization_id);
        org_role_permissions = org_perm.permissions;
    }

    if let Some(workspace_membership_id) = workspace_membership_id {
        let workspace_perm = GetWorkspaceMembershipPermissionsQuery::new(workspace_membership_id)
            .execute(&app_state)
            .await?
            .ok_or_else(|| (StatusCode::NOT_FOUND, "Workspace membership not found"))?;
        if let Some(existing_org_id) = organization_id {
            if existing_org_id != workspace_perm.organization_id {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "organization_membership_id and workspace_membership_id belong to different organizations",
                )
                    .into());
            }
        }
        organization_id = Some(workspace_perm.organization_id);
        workspace_id = Some(workspace_perm.workspace_id);
        workspace_role_permissions = workspace_perm.permissions;
    }

    if let Some(expires_at) = request.expires_at {
        command = command.with_expiration(expires_at);
    }

    command.metadata = request.metadata;

    command = command
        .with_rate_limit_scheme_slug(app.rate_limit_scheme_slug.clone())
        .with_membership_context(
            organization_id,
            workspace_id,
            org_membership_id,
            workspace_membership_id,
            org_role_permissions,
            workspace_role_permissions,
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
    let app = GetApiAuthAppBySlugQuery::new(deployment_id, app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    let keys = GetApiKeysByAppQuery::new(app.app_slug.clone(), deployment_id)
        .with_inactive(true)
        .execute(&app_state)
        .await?;
    if !keys.iter().any(|k| k.id == key_id) {
        return Err((StatusCode::NOT_FOUND, "API key not found").into());
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
    let app = GetApiAuthAppBySlugQuery::new(deployment_id, app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    let keys = GetApiKeysByAppQuery::new(app.app_slug.clone(), deployment_id)
        .with_inactive(true)
        .execute(&app_state)
        .await?;
    if !keys.iter().any(|k| k.id == key_id) {
        return Err((StatusCode::NOT_FOUND, "API key not found").into());
    }

    let command = RotateApiKeyCommand {
        key_id,
        deployment_id,
    };
    let new_key = command.execute(&app_state).await?;

    Ok(new_key.into())
}
