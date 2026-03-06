use commands::api_key_app::{
    CreateApiAuthAppCommand, DeleteApiAuthAppCommand, UpdateApiAuthAppCommand,
};
use common::db_router::ReadConsistency;
use common::state::AppState;
use dto::json::api_key::{
    CreateApiAuthAppRequest, ListApiAuthAppsQuery, ListApiAuthAppsResponse, UpdateApiAuthAppRequest,
};
use models::api_key::ApiAuthApp;
use models::error::AppError;
use models::plan_features::PlanTier;
use queries::{api_key::GetApiAuthAppsQuery, plan_access::GetDeploymentPlanTierQuery};

use super::api_key_shared::get_api_auth_app_by_slug;

pub async fn list_api_auth_apps(
    app_state: &AppState,
    deployment_id: i64,
    params: ListApiAuthAppsQuery,
) -> Result<ListApiAuthAppsResponse, AppError> {
    let include_inactive = params.include_inactive.unwrap_or(false);
    let reader = app_state.db_router.reader(ReadConsistency::Strong);

    let apps = GetApiAuthAppsQuery::new(deployment_id)
        .with_inactive(include_inactive)
        .execute_with(reader)
        .await?;

    Ok(ListApiAuthAppsResponse {
        total: apps.len(),
        apps,
    })
}

pub async fn get_api_auth_app(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
) -> Result<ApiAuthApp, AppError> {
    get_api_auth_app_by_slug(app_state, deployment_id, app_slug).await
}

pub async fn create_api_auth_app(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateApiAuthAppRequest,
) -> Result<ApiAuthApp, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let plan_tier = GetDeploymentPlanTierQuery::new(deployment_id)
        .execute_with(reader)
        .await?;

    if !matches!(plan_tier, Some(PlanTier::Growth)) {
        return Err(AppError::Forbidden(
            "API auth app creation requires Growth plan".to_string(),
        ));
    }

    if request.user_id.is_some() && (request.permissions.is_some() || request.resources.is_some()) {
        return Err(AppError::BadRequest(
            "permissions/resources cannot be set when user_id is attached".to_string(),
        ));
    }

    let mut command = CreateApiAuthAppCommand::new(
        deployment_id,
        request.user_id.map(|v| v.get()),
        request.app_slug,
        request.name,
        request.key_prefix,
    );
    command = command.with_scope(
        request.organization_id.map(|v| v.get()),
        request.workspace_id.map(|v| v.get()),
    );

    if let Some(description) = request.description {
        command = command.with_description(description);
    }

    command = command.with_rate_limit_scheme_slug(request.rate_limit_scheme_slug.clone());
    command = command.with_permissions(request.permissions.unwrap_or_default());
    command = command.with_resources(request.resources.unwrap_or_default());

    let created = command.execute_with(app_state.db_router.writer()).await?;
    get_api_auth_app_by_slug(app_state, deployment_id, created.app_slug).await
}

pub async fn update_api_auth_app(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
    request: UpdateApiAuthAppRequest,
) -> Result<ApiAuthApp, AppError> {
    let app = get_api_auth_app_by_slug(app_state, deployment_id, app_slug).await?;

    if app.user_id.is_some() && (request.permissions.is_some() || request.resources.is_some()) {
        return Err(AppError::BadRequest(
            "permissions/resources can only be updated when app is not attached to a user"
                .to_string(),
        ));
    }

    let command = UpdateApiAuthAppCommand {
        app_slug: app.app_slug.clone(),
        deployment_id,
        organization_id: request.organization_id.map(|v| v.get()),
        workspace_id: request.workspace_id.map(|v| v.get()),
        name: request.name,
        key_prefix: request.key_prefix,
        description: request.description,
        is_active: request.is_active,
        rate_limit_scheme_slug: request.rate_limit_scheme_slug.clone(),
        permissions: request.permissions.clone(),
        resources: request.resources.clone(),
    };

    let updated = command.execute_with(app_state.db_router.writer()).await?;
    get_api_auth_app_by_slug(app_state, deployment_id, updated.app_slug).await
}

pub async fn delete_api_auth_app(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
) -> Result<(), AppError> {
    let app = get_api_auth_app_by_slug(app_state, deployment_id, app_slug).await?;

    let command = DeleteApiAuthAppCommand {
        app_slug: app.app_slug.clone(),
        deployment_id,
    };
    command.execute_with(app_state.db_router.writer()).await?;

    Ok(())
}
