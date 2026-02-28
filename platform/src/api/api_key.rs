use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;

use crate::application::response::ApiResult;
use crate::middleware::RequireDeployment;
use commands::{
    Command,
    api_key::{CreateApiKeyCommand, RevokeApiKeyCommand, RotateApiKeyCommand},
    api_key_app::{CreateApiAuthAppCommand, DeleteApiAuthAppCommand, UpdateApiAuthAppCommand},
    rate_limit_scheme::{
        CreateRateLimitSchemeCommand, DeleteRateLimitSchemeCommand, UpdateRateLimitSchemeCommand,
    },
};
use common::state::AppState;
use dto::json::api_key::*;
use models::api_key::{ApiAuthApp, ApiKeyWithSecret};

use queries::{
    Query as QueryTrait,
    api_key::{
        GetApiAuthAppBySlugQuery, GetApiAuthAppsQuery, GetApiKeysByAppQuery,
        GetOrganizationMembershipIdByUserAndOrganizationQuery,
        GetOrganizationMembershipPermissionsQuery, GetWorkspaceMembershipIdByUserAndWorkspaceQuery,
        GetWorkspaceMembershipPermissionsQuery,
    },
    api_key_audit::{
        GetApiAuditAnalyticsQuery as GetApiAuditAnalyticsDataQuery,
        GetApiAuditLogsQuery as GetApiAuditLogsDataQuery,
        GetApiAuditTimeseriesQuery as GetApiAuditTimeseriesDataQuery,
    },
    rate_limit_scheme::{GetRateLimitSchemeQuery, ListRateLimitSchemesQuery, RateLimitSchemeData},
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

pub async fn list_rate_limit_schemes(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ListRateLimitSchemesResponse<RateLimitSchemeData>> {
    let schemes = ListRateLimitSchemesQuery::new(deployment_id)
        .execute(&app_state)
        .await?;

    Ok(ListRateLimitSchemesResponse {
        total: schemes.len(),
        schemes,
    }
    .into())
}

pub async fn get_rate_limit_scheme(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
) -> ApiResult<RateLimitSchemeData> {
    let scheme = GetRateLimitSchemeQuery::new(deployment_id, slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Rate limit scheme not found"))?;

    Ok(scheme.into())
}

pub async fn create_rate_limit_scheme(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateRateLimitSchemeRequest>,
) -> ApiResult<RateLimitSchemeData> {
    let scheme = CreateRateLimitSchemeCommand {
        deployment_id,
        slug: request.slug,
        name: request.name,
        description: request.description,
        rules: request.rules,
    }
    .execute(&app_state)
    .await?;

    Ok(scheme.into())
}

pub async fn update_rate_limit_scheme(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
    Json(request): Json<UpdateRateLimitSchemeRequest>,
) -> ApiResult<RateLimitSchemeData> {
    let scheme = UpdateRateLimitSchemeCommand {
        deployment_id,
        slug,
        name: request.name,
        description: request.description,
        rules: request.rules,
    }
    .execute(&app_state)
    .await?;

    Ok(scheme.into())
}

pub async fn delete_rate_limit_scheme(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(slug): Path<String>,
) -> ApiResult<()> {
    DeleteRateLimitSchemeCommand {
        deployment_id,
        slug,
    }
    .execute(&app_state)
    .await?;

    Ok((StatusCode::NO_CONTENT, ()).into())
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

pub async fn get_api_audit_logs(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<ListApiAuditLogsQuery>,
) -> ApiResult<ApiAuditLogsResponse> {
    GetApiAuthAppBySlugQuery::new(deployment_id, app_slug.clone())
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    let mut cursor_ts = params.cursor_ts;
    let mut cursor_id = params.cursor_id.clone();
    if let Some(cursor) = params.cursor {
        use base64::Engine;
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(cursor)
            .map_err(|_| (StatusCode::BAD_REQUEST, "invalid cursor"))?;
        let decoded =
            String::from_utf8(decoded).map_err(|_| (StatusCode::BAD_REQUEST, "invalid cursor"))?;
        let parts: Vec<&str> = decoded.splitn(2, '|').collect();
        if parts.len() != 2 {
            return Err((StatusCode::BAD_REQUEST, "invalid cursor").into());
        }
        let cursor_ms: i64 = parts[0]
            .parse()
            .map_err(|_| (StatusCode::BAD_REQUEST, "invalid cursor"))?;
        cursor_ts = Some(
            chrono::DateTime::from_timestamp_millis(cursor_ms)
                .ok_or((StatusCode::BAD_REQUEST, "invalid cursor"))?,
        );
        cursor_id = Some(parts[1].to_string());
    }

    let result = GetApiAuditLogsDataQuery {
        deployment_id,
        app_slug,
        limit: params.limit.unwrap_or(100),
        offset: params.offset.unwrap_or(0),
        cursor_ts,
        cursor_id,
        outcome: params.outcome,
        key_id: params.key_id.map(|v| v.get()),
        start_date: params.start_date,
        end_date: params.end_date,
    }
    .execute(&app_state)
    .await?;

    Ok(result.into())
}

pub async fn get_api_audit_analytics(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<GetApiAuditAnalyticsQuery>,
) -> ApiResult<ApiAuditAnalyticsResponse> {
    GetApiAuthAppBySlugQuery::new(deployment_id, app_slug.clone())
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    let result = GetApiAuditAnalyticsDataQuery {
        deployment_id,
        app_slug,
        start_date: params.start_date,
        end_date: params.end_date,
        key_id: params.key_id.map(|v| v.get()),
        include_top_keys: params.include_top_keys.unwrap_or(false),
        include_top_paths: params.include_top_paths.unwrap_or(false),
        include_blocked_reasons: params.include_blocked_reasons.unwrap_or(false),
        include_rate_limits: params.include_rate_limits.unwrap_or(false),
        top_limit: params.top_limit.unwrap_or(10),
    }
    .execute(&app_state)
    .await?;

    Ok(result.into())
}

pub async fn get_api_audit_timeseries(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<GetApiAuditTimeseriesQuery>,
) -> ApiResult<ApiAuditTimeseriesResponse> {
    GetApiAuthAppBySlugQuery::new(deployment_id, app_slug.clone())
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    let interval = params.interval.unwrap_or_else(|| "hour".to_string());
    let normalized_interval = match interval.as_str() {
        "minute" | "hour" | "day" | "week" | "month" => interval,
        _ => "hour".to_string(),
    };

    let result = GetApiAuditTimeseriesDataQuery {
        deployment_id,
        app_slug,
        start_date: params.start_date,
        end_date: params.end_date,
        interval: normalized_interval,
        key_id: params.key_id.map(|v| v.get()),
    }
    .execute(&app_state)
    .await?;

    Ok(result.into())
}
