use axum::http::StatusCode;

use crate::application::response::ApiErrorResponse;
use common::state::AppState;
use models::api_key::ApiAuthApp;
use queries::{
    Query as QueryTrait,
    api_key::{
        GetApiAuthAppBySlugQuery, GetApiKeysByAppQuery,
        GetOrganizationMembershipIdByUserAndOrganizationQuery,
        GetOrganizationMembershipPermissionsQuery, GetWorkspaceMembershipIdByUserAndWorkspaceQuery,
        GetWorkspaceMembershipPermissionsQuery,
    },
};

#[derive(Debug, Default)]
pub(super) struct ApiKeyMembershipContext {
    pub(super) organization_id: Option<i64>,
    pub(super) workspace_id: Option<i64>,
    pub(super) organization_membership_id: Option<i64>,
    pub(super) workspace_membership_id: Option<i64>,
    pub(super) org_role_permissions: Vec<String>,
    pub(super) workspace_role_permissions: Vec<String>,
}

pub(super) async fn get_api_auth_app_by_slug(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
) -> Result<ApiAuthApp, ApiErrorResponse> {
    GetApiAuthAppBySlugQuery::new(deployment_id, app_slug)
        .execute(app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found").into())
}

pub(super) async fn ensure_api_key_exists_for_app(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: &str,
    key_id: i64,
) -> Result<(), ApiErrorResponse> {
    let keys = GetApiKeysByAppQuery::new(app_slug.to_string(), deployment_id)
        .with_inactive(true)
        .execute(app_state)
        .await?;
    if keys.iter().any(|key| key.id == key_id) {
        Ok(())
    } else {
        Err((StatusCode::NOT_FOUND, "API key not found").into())
    }
}

pub(super) async fn resolve_api_key_membership_context(
    app_state: &AppState,
    app: &ApiAuthApp,
) -> Result<ApiKeyMembershipContext, ApiErrorResponse> {
    let mut context = ApiKeyMembershipContext::default();

    if let (Some(user_id), Some(organization_id)) = (app.user_id, app.organization_id) {
        context.organization_membership_id =
            GetOrganizationMembershipIdByUserAndOrganizationQuery::new(user_id, organization_id)
                .execute(app_state)
                .await?;
        if context.organization_membership_id.is_none() {
            return Err((StatusCode::BAD_REQUEST, "user is not a member of the org").into());
        }
    }

    if let (Some(user_id), Some(workspace_id)) = (app.user_id, app.workspace_id) {
        context.workspace_membership_id =
            GetWorkspaceMembershipIdByUserAndWorkspaceQuery::new(user_id, workspace_id)
                .execute(app_state)
                .await?;
        if context.workspace_membership_id.is_none() {
            return Err((
                StatusCode::BAD_REQUEST,
                "user is not a member of the org",
            )
                .into());
        }
    }

    if let Some(organization_membership_id) = context.organization_membership_id {
        let organization_permissions =
            GetOrganizationMembershipPermissionsQuery::new(organization_membership_id)
                .execute(app_state)
                .await?
                .ok_or_else(|| (StatusCode::NOT_FOUND, "Organization membership not found"))?;
        context.organization_id = Some(organization_permissions.organization_id);
        context.org_role_permissions = organization_permissions.permissions;
    }

    if let Some(workspace_membership_id) = context.workspace_membership_id {
        let workspace_permissions =
            GetWorkspaceMembershipPermissionsQuery::new(workspace_membership_id)
                .execute(app_state)
                .await?
                .ok_or_else(|| (StatusCode::NOT_FOUND, "Workspace membership not found"))?;

        if let Some(existing_organization_id) = context.organization_id {
            if existing_organization_id != workspace_permissions.organization_id {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "organization_membership_id and workspace_membership_id belong to different organizations",
                )
                    .into());
            }
        }

        context.organization_id = Some(workspace_permissions.organization_id);
        context.workspace_id = Some(workspace_permissions.workspace_id);
        context.workspace_role_permissions = workspace_permissions.permissions;
    }

    Ok(context)
}
