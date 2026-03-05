use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;

use crate::application::response::ApiResult;
use crate::middleware::RequireDeployment;
use commands::{
    Command,
    api_key_app::{CreateApiAuthAppCommand, DeleteApiAuthAppCommand, UpdateApiAuthAppCommand},
};
use common::state::AppState;
use dto::json::api_key::*;
use models::api_key::ApiAuthApp;
use models::plan_features::PlanTier;
use queries::{
    Query as QueryTrait,
    api_key::{GetApiAuthAppBySlugQuery, GetApiAuthAppsQuery},
    plan_access::GetDeploymentPlanTierQuery,
};

pub async fn list_api_auth_apps(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ListApiAuthAppsQuery>,
) -> ApiResult<ListApiAuthAppsResponse> {
    let include_inactive = params.include_inactive.unwrap_or(false);

    let apps = GetApiAuthAppsQuery::new(deployment_id)
        .with_inactive(include_inactive)
        .execute(&app_state)
        .await?;

    Ok(ListApiAuthAppsResponse {
        total: apps.len(),
        apps,
    }
    .into())
}

pub async fn get_api_auth_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
) -> ApiResult<ApiAuthApp> {
    let app = GetApiAuthAppBySlugQuery::new(deployment_id, app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    Ok(app.into())
}

pub async fn create_api_auth_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateApiAuthAppRequest>,
) -> ApiResult<ApiAuthApp> {
    let plan_tier = GetDeploymentPlanTierQuery::new(deployment_id)
        .execute(&app_state)
        .await?;
    if !matches!(plan_tier, Some(PlanTier::Growth)) {
        return Err((
            StatusCode::FORBIDDEN,
            "API auth app creation requires Growth plan",
        )
            .into());
    }

    if request.user_id.is_some() && (request.permissions.is_some() || request.resources.is_some()) {
        return Err((
            StatusCode::BAD_REQUEST,
            "permissions/resources cannot be set when user_id is attached",
        )
            .into());
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

    let created = command.execute(&app_state).await?;
    let app = GetApiAuthAppBySlugQuery::new(deployment_id, created.app_slug.clone())
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    Ok(app.into())
}

pub async fn update_api_auth_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Json(request): Json<UpdateApiAuthAppRequest>,
) -> ApiResult<ApiAuthApp> {
    let app = GetApiAuthAppBySlugQuery::new(deployment_id, app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

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

    if app.user_id.is_some() && (request.permissions.is_some() || request.resources.is_some()) {
        return Err((
            StatusCode::BAD_REQUEST,
            "permissions/resources can only be updated when app is not attached to a user",
        )
            .into());
    }

    let updated = command.execute(&app_state).await?;

    let app = GetApiAuthAppBySlugQuery::new(deployment_id, updated.app_slug.clone())
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    Ok(app.into())
}

pub async fn delete_api_auth_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
) -> ApiResult<()> {
    // First get the app by name to find its ID
    let app = GetApiAuthAppBySlugQuery::new(deployment_id, app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    let command = DeleteApiAuthAppCommand {
        app_slug: app.app_slug.clone(),
        deployment_id,
    };
    command.execute(&app_state).await?;

    Ok(().into())
}
