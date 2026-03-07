use commands::oauth::UpdateOAuthAppCommand;
use common::db_router::ReadConsistency;
use common::state::AppState;
use dto::json::api_key::{OAuthAppResponse, SetOAuthScopeMappingRequest, UpdateOAuthScopeRequest};
use models::api_key::OAuthScopeDefinition;
use models::error::AppError;
use queries::GetDeploymentWithSettingsQuery;

use super::oauth_shared::{get_oauth_app_by_slug, map_oauth_app_response};

fn normalized_scope(scope: &str) -> Result<String, AppError> {
    let scope = scope.trim().to_string();
    if scope.is_empty() {
        Err(AppError::BadRequest("scope is required".to_string()))
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
) -> Result<OAuthAppResponse, AppError> {
    let writer = app_state.db_router.writer();
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
    .execute_with_db(writer)
    .await?;

    Ok(map_oauth_app_response(updated))
}

pub async fn update_oauth_scope(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
    scope: String,
    request: UpdateOAuthScopeRequest,
) -> Result<OAuthAppResponse, AppError> {
    let scope = normalized_scope(&scope)?;

    let oauth_app = get_oauth_app_by_slug(app_state, deployment_id, oauth_app_slug).await?;
    let mut scope_definitions = oauth_app.scope_definitions_vec();
    let scope_definition = scope_definitions
        .iter_mut()
        .find(|definition| definition.scope == scope)
        .ok_or_else(|| AppError::NotFound("scope definition not found".to_string()))?;

    if let Some(display_name) = request.display_name {
        scope_definition.display_name = display_name.trim().to_string();
    }
    if let Some(description) = request.description {
        scope_definition.description = description.trim().to_string();
    }
    let supported_scopes = oauth_app.supported_scopes_vec();

    persist_scope_updates(
        app_state,
        deployment_id,
        oauth_app.slug,
        supported_scopes,
        scope_definitions,
    )
    .await
}

pub async fn archive_oauth_scope(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
    scope: String,
) -> Result<OAuthAppResponse, AppError> {
    set_oauth_scope_archived(app_state, deployment_id, oauth_app_slug, scope, true).await
}

pub async fn unarchive_oauth_scope(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
    scope: String,
) -> Result<OAuthAppResponse, AppError> {
    set_oauth_scope_archived(app_state, deployment_id, oauth_app_slug, scope, false).await
}

pub async fn set_oauth_scope_mapping(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
    scope: String,
    request: SetOAuthScopeMappingRequest,
) -> Result<OAuthAppResponse, AppError> {
    let scope = normalized_scope(&scope)?;

    let category = request.category.trim().to_ascii_lowercase();
    if !matches!(category.as_str(), "personal" | "organization" | "workspace") {
        return Err(AppError::BadRequest(
            "category must be personal, organization, or workspace".to_string(),
        ));
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
        return Err(AppError::BadRequest(
            "personal category cannot map organization/workspace permissions".to_string(),
        ));
    }
    if category == "organization" && workspace_permission.is_some() {
        return Err(AppError::BadRequest(
            "organization category cannot map workspace permission".to_string(),
        ));
    }
    if category == "workspace" && organization_permission.is_some() {
        return Err(AppError::BadRequest(
            "workspace category cannot map organization permission".to_string(),
        ));
    }

    if organization_permission.is_some() || workspace_permission.is_some() {
        let reader = app_state.db_router.reader(ReadConsistency::Strong);
        let deployment = GetDeploymentWithSettingsQuery::new(deployment_id)
            .execute_with_db(reader)
            .await?;

        if let Some(permission) = organization_permission.as_deref() {
            let available_permissions = deployment
                .b2b_settings
                .as_ref()
                .and_then(|settings| settings.settings.organization_permissions.as_ref())
                .cloned()
                .unwrap_or_default();

            if !available_permissions.iter().any(|p| p == permission) {
                return Err(AppError::BadRequest(format!(
                    "organization permission '{}' is not configured in deployment B2B settings",
                    permission
                )));
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
                return Err(AppError::BadRequest(format!(
                    "workspace permission '{}' is not configured in deployment B2B settings",
                    permission
                )));
            }
        }
    }

    let oauth_app = get_oauth_app_by_slug(app_state, deployment_id, oauth_app_slug).await?;
    let mut scope_definitions = oauth_app.scope_definitions_vec();
    let scope_definition = scope_definitions
        .iter_mut()
        .find(|definition| definition.scope == scope)
        .ok_or_else(|| AppError::NotFound("scope definition not found".to_string()))?;

    if !scope_definition.category.trim().is_empty() && scope_definition.category != category {
        return Err(AppError::BadRequest(
            "scope category is immutable once set".to_string(),
        ));
    }
    if scope_definition.organization_permission.is_some()
        && scope_definition.organization_permission.as_deref() != organization_permission.as_deref()
    {
        return Err(AppError::BadRequest(
            "organization permission is immutable once set".to_string(),
        ));
    }
    if scope_definition.workspace_permission.is_some()
        && scope_definition.workspace_permission.as_deref() != workspace_permission.as_deref()
    {
        return Err(AppError::BadRequest(
            "workspace permission is immutable once set".to_string(),
        ));
    }

    scope_definition.category = category;
    scope_definition.organization_permission = organization_permission;
    scope_definition.workspace_permission = workspace_permission;
    let supported_scopes = oauth_app.supported_scopes_vec();

    persist_scope_updates(
        app_state,
        deployment_id,
        oauth_app.slug,
        supported_scopes,
        scope_definitions,
    )
    .await
}

async fn set_oauth_scope_archived(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
    scope: String,
    archived: bool,
) -> Result<OAuthAppResponse, AppError> {
    let scope = normalized_scope(&scope)?;

    let oauth_app = get_oauth_app_by_slug(app_state, deployment_id, oauth_app_slug).await?;
    let mut scope_definitions = oauth_app.scope_definitions_vec();
    let scope_definition = scope_definitions
        .iter_mut()
        .find(|definition| definition.scope == scope)
        .ok_or_else(|| AppError::NotFound("scope definition not found".to_string()))?;
    scope_definition.archived = archived;
    let supported_scopes = oauth_app.supported_scopes_vec();

    persist_scope_updates(
        app_state,
        deployment_id,
        oauth_app.slug,
        supported_scopes,
        scope_definitions,
    )
    .await
}
