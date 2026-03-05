use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::application::response::ApiResult;
use crate::middleware::RequireDeployment;
use commands::{Command, oauth::UpdateOAuthAppCommand};
use common::state::AppState;
use dto::json::api_key::{OAuthAppResponse, SetOAuthScopeMappingRequest, UpdateOAuthScopeRequest};
use models::api_key::OAuthScopeDefinition;
use queries::{GetDeploymentWithSettingsQuery, Query as QueryTrait};

use super::helpers::get_oauth_app_by_slug;
use super::mappers::map_oauth_app_response;
use super::types::OAuthScopePathParams;

fn normalized_scope(scope: &str) -> Result<String, (StatusCode, &'static str)> {
    let scope = scope.trim().to_string();
    if scope.is_empty() {
        Err((StatusCode::BAD_REQUEST, "scope is required"))
    } else {
        Ok(scope)
    }
}

async fn persist_scope_updates(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
    supported_scopes: Vec<String>,
    scope_definitions: Vec<OAuthScopeDefinition>,
) -> Result<OAuthAppResponse, crate::application::response::ApiErrorResponse> {
    let updated = UpdateOAuthAppCommand {
        deployment_id,
        oauth_app_slug,
        name: None,
        description: None,
        supported_scopes: Some(supported_scopes),
        scope_definitions: Some(scope_definitions),
        allow_dynamic_client_registration: None,
        is_active: None,
    }
    .execute(app_state)
    .await?;

    Ok(map_oauth_app_response(updated))
}

pub(crate) async fn update_oauth_scope(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthScopePathParams>,
    Json(request): Json<UpdateOAuthScopeRequest>,
) -> ApiResult<OAuthAppResponse> {
    let scope = normalized_scope(&params.scope)?;

    let oauth_app = get_oauth_app_by_slug(&app_state, deployment_id, params.oauth_app_slug).await?;

    let mut scope_definitions = oauth_app.scope_definitions_vec();
    let scope_definition = scope_definitions
        .iter_mut()
        .find(|definition| definition.scope == scope)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "scope definition not found"))?;

    if let Some(display_name) = request.display_name {
        scope_definition.display_name = display_name.trim().to_string();
    }
    if let Some(description) = request.description {
        scope_definition.description = description.trim().to_string();
    }
    let supported_scopes = oauth_app.supported_scopes_vec();
    let response = persist_scope_updates(
        &app_state,
        deployment_id,
        oauth_app.slug,
        supported_scopes,
        scope_definitions,
    )
    .await?;

    Ok(response.into())
}

pub(crate) async fn archive_oauth_scope(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthScopePathParams>,
) -> ApiResult<OAuthAppResponse> {
    set_oauth_scope_archived(&app_state, deployment_id, params, true).await
}

pub(crate) async fn unarchive_oauth_scope(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthScopePathParams>,
) -> ApiResult<OAuthAppResponse> {
    set_oauth_scope_archived(&app_state, deployment_id, params, false).await
}

pub(crate) async fn set_oauth_scope_mapping(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthScopePathParams>,
    Json(request): Json<SetOAuthScopeMappingRequest>,
) -> ApiResult<OAuthAppResponse> {
    let scope = normalized_scope(&params.scope)?;

    let category = request.category.trim().to_ascii_lowercase();
    if !matches!(category.as_str(), "personal" | "organization" | "workspace") {
        return Err((
            StatusCode::BAD_REQUEST,
            "category must be personal, organization, or workspace",
        )
            .into());
    }

    let organization_permission = request
        .organization_permission
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let workspace_permission = request
        .workspace_permission
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    if category == "personal"
        && (organization_permission.is_some() || workspace_permission.is_some())
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "personal category cannot map organization/workspace permissions",
        )
            .into());
    }
    if category == "organization" && workspace_permission.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            "organization category cannot map workspace permission",
        )
            .into());
    }
    if category == "workspace" && organization_permission.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            "workspace category cannot map organization permission",
        )
            .into());
    }

    if organization_permission.is_some() || workspace_permission.is_some() {
        let deployment = GetDeploymentWithSettingsQuery::new(deployment_id)
            .execute(&app_state)
            .await?;

        if let Some(permission) = organization_permission.as_deref() {
            let available_permissions = deployment
                .b2b_settings
                .as_ref()
                .and_then(|settings| settings.settings.organization_permissions.as_ref())
                .cloned()
                .unwrap_or_default();
            if !available_permissions.iter().any(|p| p == permission) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!(
                        "organization permission '{}' is not configured in deployment B2B settings",
                        permission
                    ),
                )
                    .into());
            }
        }

        if let Some(permission) = workspace_permission.as_deref() {
            let available_permissions = deployment
                .b2b_settings
                .as_ref()
                .and_then(|settings| settings.settings.workspace_permissions.as_ref())
                .cloned()
                .unwrap_or_default();
            if !available_permissions.iter().any(|p| p == permission) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!(
                        "workspace permission '{}' is not configured in deployment B2B settings",
                        permission
                    ),
                )
                    .into());
            }
        }
    }

    let oauth_app = get_oauth_app_by_slug(&app_state, deployment_id, params.oauth_app_slug).await?;

    let mut scope_definitions = oauth_app.scope_definitions_vec();
    let scope_definition = scope_definitions
        .iter_mut()
        .find(|definition| definition.scope == scope)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "scope definition not found"))?;

    if !scope_definition.category.trim().is_empty() && scope_definition.category != category {
        return Err((
            StatusCode::BAD_REQUEST,
            "scope category is immutable once set",
        )
            .into());
    }
    if scope_definition.organization_permission.is_some()
        && scope_definition.organization_permission.as_deref() != organization_permission.as_deref()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "organization permission is immutable once set",
        )
            .into());
    }
    if scope_definition.workspace_permission.is_some()
        && scope_definition.workspace_permission.as_deref() != workspace_permission.as_deref()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "workspace permission is immutable once set",
        )
            .into());
    }

    scope_definition.category = category;
    scope_definition.organization_permission = organization_permission;
    scope_definition.workspace_permission = workspace_permission;

    let supported_scopes = oauth_app.supported_scopes_vec();
    let response = persist_scope_updates(
        &app_state,
        deployment_id,
        oauth_app.slug,
        supported_scopes,
        scope_definitions,
    )
    .await?;

    Ok(response.into())
}

async fn set_oauth_scope_archived(
    app_state: &AppState,
    deployment_id: i64,
    params: OAuthScopePathParams,
    archived: bool,
) -> ApiResult<OAuthAppResponse> {
    let scope = normalized_scope(&params.scope)?;

    let oauth_app = get_oauth_app_by_slug(app_state, deployment_id, params.oauth_app_slug).await?;

    let mut scope_definitions = oauth_app.scope_definitions_vec();
    let scope_definition = scope_definitions
        .iter_mut()
        .find(|definition| definition.scope == scope)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "scope definition not found"))?;
    scope_definition.archived = archived;

    let supported_scopes = oauth_app.supported_scopes_vec();
    let response = persist_scope_updates(
        app_state,
        deployment_id,
        oauth_app.slug,
        supported_scopes,
        scope_definitions,
    )
    .await?;

    Ok(response.into())
}
