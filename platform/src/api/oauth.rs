use axum::Json;
use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use models::api_key::OAuthScopeDefinition;
use serde::Deserialize;

use crate::application::response::ApiResult;
use crate::middleware::RequireDeployment;
use commands::{
    Command, UploadToCdnCommand,
    oauth::{
        CreateOAuthAppCommand, CreateOAuthClientCommand, DeactivateOAuthClient,
        RevokeOAuthClientGrantCommand, RotateOAuthClientSecret, UpdateOAuthAppCommand,
        UpdateOAuthClientSettings,
    },
};
use common::state::AppState;
use dto::json::api_key::{
    CreateOAuthClientRequest, ListOAuthAppsResponse, ListOAuthClientsResponse,
    ListOAuthGrantsResponse, OAuthAppResponse, OAuthClientResponse, OAuthGrantResponse,
    RotateOAuthClientSecretResponse, SetOAuthScopeMappingRequest, UpdateOAuthAppRequest,
    UpdateOAuthClientRequest, UpdateOAuthScopeRequest,
};
use queries::{
    GetDeploymentWithSettingsQuery,
    Query as QueryTrait,
    oauth::{
        GetOAuthAppBySlugQuery, GetOAuthClientByIdQuery, ListOAuthAppsByDeploymentQuery,
        ListOAuthClientsByOAuthAppQuery, ListOAuthGrantsByClientQuery,
    },
};

#[derive(Debug, Deserialize)]
pub(crate) struct OAuthAppPathParams {
    oauth_app_slug: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OAuthClientPathParams {
    oauth_app_slug: String,
    oauth_client_id: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OAuthGrantPathParams {
    oauth_app_slug: String,
    oauth_client_id: i64,
    grant_id: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OAuthScopePathParams {
    oauth_app_slug: String,
    scope: String,
}

pub async fn list_oauth_apps(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ListOAuthAppsResponse> {
    let apps = ListOAuthAppsByDeploymentQuery::new(deployment_id)
        .execute(&app_state)
        .await?
        .into_iter()
        .map(|a| {
            let supported_scopes = a.supported_scopes_vec();
            let scope_definitions = a.scope_definitions_vec();
            OAuthAppResponse {
                id: a.id,
                slug: a.slug,
                name: a.name,
                description: a.description,
                logo_url: a.logo_url,
                fqdn: a.fqdn,
                supported_scopes,
                scope_definitions,
                allow_dynamic_client_registration: a.allow_dynamic_client_registration,
                is_active: a.is_active,
                created_at: a.created_at,
                updated_at: a.updated_at,
            }
        })
        .collect();

    Ok(ListOAuthAppsResponse { apps }.into())
}

pub async fn create_oauth_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    mut multipart: Multipart,
) -> ApiResult<OAuthAppResponse> {
    let mut slug: Option<String> = None;
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut fqdn: Option<String> = None;
    let mut supported_scopes: Vec<String> = vec![];
    let mut scope_definitions: Option<Vec<OAuthScopeDefinition>> = None;
    let mut allow_dynamic_client_registration = false;
    let mut logo_image_data: Option<(Vec<u8>, String)> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let field_name = field.name().unwrap_or_default().to_string();

        match field_name.as_str() {
            "slug" => {
                let value = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                let trimmed = value.trim().to_string();
                if !trimmed.is_empty() {
                    slug = Some(trimmed);
                }
            }
            "name" => {
                let value = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                let trimmed = value.trim().to_string();
                if !trimmed.is_empty() {
                    name = Some(trimmed);
                }
            }
            "description" => {
                let value = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                let trimmed = value.trim().to_string();
                if !trimmed.is_empty() {
                    description = Some(trimmed);
                }
            }
            "fqdn" | "domain" => {
                let value = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                let trimmed = value.trim().to_string();
                if !trimmed.is_empty() {
                    fqdn = Some(trimmed);
                }
            }
            "supported_scopes" => {
                let value = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                supported_scopes = value
                    .split(',')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
            }
            "scope_definitions" => {
                let value = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                let parsed: Vec<OAuthScopeDefinition> =
                    serde_json::from_str(&value).map_err(|_| {
                        (
                            StatusCode::BAD_REQUEST,
                            "scope_definitions must be valid JSON array".to_string(),
                        )
                    })?;
                scope_definitions = Some(parsed);
            }
            "allow_dynamic_client_registration" => {
                let value = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                allow_dynamic_client_registration = matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                );
            }
            "logo" => {
                let content_type = field.content_type().unwrap_or_default().to_string();

                if content_type.starts_with("image/") {
                    let file_extension =
                        if content_type == "image/jpeg" || content_type == "image/jpg" {
                            "jpg"
                        } else if content_type == "image/png" {
                            "png"
                        } else if content_type == "image/gif" {
                            "gif"
                        } else if content_type == "image/webp" {
                            "webp"
                        } else if content_type == "image/x-icon"
                            || content_type == "image/vnd.microsoft.icon"
                        {
                            "ico"
                        } else {
                            return Err((
                            StatusCode::BAD_REQUEST,
                            "Unsupported image format. Supported formats: JPEG, PNG, GIF, WEBP, ICO"
                                .to_string(),
                        )
                            .into());
                        };

                    let image_buffer = field
                        .bytes()
                        .await
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                        .to_vec();

                    if !image_buffer.is_empty() {
                        logo_image_data = Some((image_buffer, file_extension.to_string()));
                    }
                }
            }
            _ => {}
        }
    }

    let slug = slug.ok_or_else(|| (StatusCode::BAD_REQUEST, "slug is required"))?;
    let name = name.ok_or_else(|| (StatusCode::BAD_REQUEST, "name is required"))?;

    let logo_url = if let Some((image_buffer, file_extension)) = logo_image_data {
        let file_path = format!(
            "deployments/{}/oauth-apps/{}/logo.{}",
            deployment_id, slug, file_extension
        );
        Some(
            UploadToCdnCommand::new(file_path, image_buffer)
                .execute(&app_state)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        )
    } else {
        None
    };

    let created = CreateOAuthAppCommand {
        deployment_id,
        slug,
        name,
        description,
        logo_url,
        fqdn,
        supported_scopes,
        scope_definitions,
        allow_dynamic_client_registration,
    }
    .execute(&app_state)
    .await?;

    let supported_scopes = created.supported_scopes_vec();
    let scope_definitions = created.scope_definitions_vec();
    Ok(OAuthAppResponse {
        id: created.id,
        slug: created.slug,
        name: created.name,
        description: created.description,
        logo_url: created.logo_url,
        fqdn: created.fqdn,
        supported_scopes,
        scope_definitions,
        allow_dynamic_client_registration: created.allow_dynamic_client_registration,
        is_active: created.is_active,
        created_at: created.created_at,
        updated_at: created.updated_at,
    }
    .into())
}

pub(crate) async fn update_oauth_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthAppPathParams>,
    Json(request): Json<UpdateOAuthAppRequest>,
) -> ApiResult<OAuthAppResponse> {
    let updated = UpdateOAuthAppCommand {
        deployment_id,
        oauth_app_slug: params.oauth_app_slug,
        name: request.name,
        description: request.description,
        supported_scopes: request.supported_scopes,
        scope_definitions: request.scope_definitions,
        allow_dynamic_client_registration: request.allow_dynamic_client_registration,
        is_active: request.is_active,
    }
    .execute(&app_state)
    .await?;

    let supported_scopes = updated.supported_scopes_vec();
    let scope_definitions = updated.scope_definitions_vec();
    Ok(OAuthAppResponse {
        id: updated.id,
        slug: updated.slug,
        name: updated.name,
        description: updated.description,
        logo_url: updated.logo_url,
        fqdn: updated.fqdn,
        supported_scopes,
        scope_definitions,
        allow_dynamic_client_registration: updated.allow_dynamic_client_registration,
        is_active: updated.is_active,
        created_at: updated.created_at,
        updated_at: updated.updated_at,
    }
    .into())
}

pub(crate) async fn update_oauth_scope(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthScopePathParams>,
    Json(request): Json<UpdateOAuthScopeRequest>,
) -> ApiResult<OAuthAppResponse> {
    let scope = params.scope.trim().to_string();
    if scope.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "scope is required").into());
    }

    let oauth_app = GetOAuthAppBySlugQuery::new(deployment_id, params.oauth_app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found"))?;

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
    let oauth_app_slug = oauth_app.slug.clone();
    let supported_scopes = oauth_app.supported_scopes_vec();
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
    .execute(&app_state)
    .await?;

    Ok(map_oauth_app_response(updated).into())
}

pub(crate) async fn archive_oauth_scope(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthScopePathParams>,
) -> ApiResult<OAuthAppResponse> {
    set_oauth_scope_archived(app_state, deployment_id, params, true).await
}

pub(crate) async fn unarchive_oauth_scope(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthScopePathParams>,
) -> ApiResult<OAuthAppResponse> {
    set_oauth_scope_archived(app_state, deployment_id, params, false).await
}

pub(crate) async fn set_oauth_scope_mapping(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthScopePathParams>,
    Json(request): Json<SetOAuthScopeMappingRequest>,
) -> ApiResult<OAuthAppResponse> {
    let scope = params.scope.trim().to_string();
    if scope.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "scope is required").into());
    }

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

    if category == "personal" && (organization_permission.is_some() || workspace_permission.is_some())
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

    if let Some(permission) = organization_permission.as_deref() {
        let deployment = GetDeploymentWithSettingsQuery::new(deployment_id)
            .execute(&app_state)
            .await?;
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
        let deployment = GetDeploymentWithSettingsQuery::new(deployment_id)
            .execute(&app_state)
            .await?;
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

    let oauth_app = GetOAuthAppBySlugQuery::new(deployment_id, params.oauth_app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found"))?;

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

    let oauth_app_slug = oauth_app.slug.clone();
    let supported_scopes = oauth_app.supported_scopes_vec();
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
    .execute(&app_state)
    .await?;

    Ok(map_oauth_app_response(updated).into())
}

pub(crate) async fn list_oauth_clients(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthAppPathParams>,
) -> ApiResult<ListOAuthClientsResponse> {
    let oauth_app = GetOAuthAppBySlugQuery::new(deployment_id, params.oauth_app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found"))?;

    let clients = ListOAuthClientsByOAuthAppQuery::new(deployment_id, oauth_app.id)
        .execute(&app_state)
        .await?
        .into_iter()
        .map(map_oauth_client_response)
        .collect();

    Ok(ListOAuthClientsResponse { clients }.into())
}

pub(crate) async fn create_oauth_client(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthAppPathParams>,
    Json(request): Json<CreateOAuthClientRequest>,
) -> ApiResult<OAuthClientResponse> {
    let oauth_app = GetOAuthAppBySlugQuery::new(deployment_id, params.oauth_app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found"))?;

    let created = CreateOAuthClientCommand {
        deployment_id,
        oauth_app_id: oauth_app.id,
        client_auth_method: request.client_auth_method,
        grant_types: request.grant_types,
        redirect_uris: request.redirect_uris,
        client_name: request.client_name,
        client_uri: request.client_uri,
        logo_uri: request.logo_uri,
        tos_uri: request.tos_uri,
        policy_uri: request.policy_uri,
        contacts: request.contacts,
        software_id: request.software_id,
        software_version: request.software_version,
        token_endpoint_auth_signing_alg: request.token_endpoint_auth_signing_alg,
        jwks_uri: request.jwks_uri,
        jwks: request.jwks,
        public_key_pem: request.public_key_pem,
    }
    .execute(&app_state)
    .await?;

    let grant_types = created.client.grant_types_vec();
    let redirect_uris = created.client.redirect_uris_vec();
    let contacts = created.client.contacts_vec();

    Ok(OAuthClientResponse {
        id: created.client.id,
        oauth_app_id: created.client.oauth_app_id,
        client_id: created.client.client_id,
        client_auth_method: created.client.client_auth_method,
        grant_types,
        redirect_uris,
        client_name: created.client.client_name,
        client_uri: created.client.client_uri,
        logo_uri: created.client.logo_uri,
        tos_uri: created.client.tos_uri,
        policy_uri: created.client.policy_uri,
        contacts,
        software_id: created.client.software_id,
        software_version: created.client.software_version,
        token_endpoint_auth_signing_alg: created.client.token_endpoint_auth_signing_alg,
        jwks_uri: created.client.jwks_uri,
        jwks: created.client.jwks,
        public_key_pem: created.client.public_key_pem,
        is_active: created.client.is_active,
        created_at: created.client.created_at,
        updated_at: created.client.updated_at,
        client_secret: created.client_secret,
    }
    .into())
}

pub(crate) async fn update_oauth_client(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthClientPathParams>,
    Json(request): Json<UpdateOAuthClientRequest>,
) -> ApiResult<OAuthClientResponse> {
    let oauth_app = GetOAuthAppBySlugQuery::new(deployment_id, params.oauth_app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found"))?;

    let client = GetOAuthClientByIdQuery::new(deployment_id, oauth_app.id, params.oauth_client_id)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;

    let updated = UpdateOAuthClientSettings {
        oauth_app_id: oauth_app.id,
        client_id: client.client_id,
        client_auth_method: request.client_auth_method,
        grant_types: request.grant_types,
        redirect_uris: request.redirect_uris,
        client_name: request.client_name,
        client_uri: request.client_uri,
        logo_uri: request.logo_uri,
        tos_uri: request.tos_uri,
        policy_uri: request.policy_uri,
        contacts: request.contacts,
        software_id: request.software_id,
        software_version: request.software_version,
        token_endpoint_auth_signing_alg: request.token_endpoint_auth_signing_alg,
        jwks_uri: request.jwks_uri,
        jwks: request.jwks,
        public_key_pem: request.public_key_pem,
    }
    .execute(&app_state)
    .await?
    .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found or inactive"))?;

    Ok(map_oauth_client_response(updated).into())
}

pub(crate) async fn deactivate_oauth_client(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthClientPathParams>,
) -> ApiResult<()> {
    let oauth_app = GetOAuthAppBySlugQuery::new(deployment_id, params.oauth_app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found"))?;

    let client = GetOAuthClientByIdQuery::new(deployment_id, oauth_app.id, params.oauth_client_id)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;

    let updated = DeactivateOAuthClient {
        oauth_app_id: oauth_app.id,
        client_id: client.client_id,
    }
    .execute(&app_state)
    .await?;

    if !updated {
        return Err((
            StatusCode::NOT_FOUND,
            "OAuth client not found or already inactive",
        )
            .into());
    }

    Ok((StatusCode::NO_CONTENT, ()).into())
}

pub(crate) async fn rotate_oauth_client_secret(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthClientPathParams>,
) -> ApiResult<RotateOAuthClientSecretResponse> {
    let oauth_app = GetOAuthAppBySlugQuery::new(deployment_id, params.oauth_app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found"))?;

    let client = GetOAuthClientByIdQuery::new(deployment_id, oauth_app.id, params.oauth_client_id)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;

    let client_secret = RotateOAuthClientSecret {
        oauth_app_id: oauth_app.id,
        client_id: client.client_id,
    }
    .execute(&app_state)
    .await?
    .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found or inactive"))?;

    Ok(RotateOAuthClientSecretResponse { client_secret }.into())
}

pub(crate) async fn list_oauth_grants(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthClientPathParams>,
) -> ApiResult<ListOAuthGrantsResponse> {
    let oauth_app = GetOAuthAppBySlugQuery::new(deployment_id, params.oauth_app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found"))?;

    let client = GetOAuthClientByIdQuery::new(deployment_id, oauth_app.id, params.oauth_client_id)
        .execute(&app_state)
        .await?;
    if client.is_none() {
        return Err((StatusCode::NOT_FOUND, "OAuth client not found").into());
    }

    let grants = ListOAuthGrantsByClientQuery::new(deployment_id, params.oauth_client_id)
        .execute(&app_state)
        .await?
        .into_iter()
        .map(|g| {
            let scopes = g.scopes_vec();

            OAuthGrantResponse {
                id: g.id,
                api_auth_app_slug: g.api_auth_app_slug,
                oauth_client_id: g.oauth_client_id,
                resource: g.resource,
                scopes,
                status: g.status,
                granted_at: g.granted_at,
                expires_at: g.expires_at,
                revoked_at: g.revoked_at,
                granted_by_user_id: g.granted_by_user_id,
                created_at: g.created_at,
                updated_at: g.updated_at,
            }
        })
        .collect();

    Ok(ListOAuthGrantsResponse { grants }.into())
}

pub(crate) async fn revoke_oauth_grant(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthGrantPathParams>,
) -> ApiResult<()> {
    let oauth_app = GetOAuthAppBySlugQuery::new(deployment_id, params.oauth_app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found"))?;

    let client = GetOAuthClientByIdQuery::new(deployment_id, oauth_app.id, params.oauth_client_id)
        .execute(&app_state)
        .await?;
    if client.is_none() {
        return Err((StatusCode::NOT_FOUND, "OAuth client not found").into());
    }

    RevokeOAuthClientGrantCommand {
        deployment_id,
        oauth_client_id: params.oauth_client_id,
        grant_id: params.grant_id,
    }
    .execute(&app_state)
    .await?;

    Ok(().into())
}

fn map_oauth_client_response(c: queries::oauth::OAuthClientData) -> OAuthClientResponse {
    let grant_types = c.grant_types_vec();
    let redirect_uris = c.redirect_uris_vec();
    let contacts = c.contacts_vec();
    OAuthClientResponse {
        id: c.id,
        oauth_app_id: c.oauth_app_id,
        client_id: c.client_id,
        client_auth_method: c.client_auth_method,
        grant_types,
        redirect_uris,
        client_name: c.client_name,
        client_uri: c.client_uri,
        logo_uri: c.logo_uri,
        tos_uri: c.tos_uri,
        policy_uri: c.policy_uri,
        contacts,
        software_id: c.software_id,
        software_version: c.software_version,
        token_endpoint_auth_signing_alg: c.token_endpoint_auth_signing_alg,
        jwks_uri: c.jwks_uri,
        jwks: c.jwks,
        public_key_pem: c.public_key_pem,
        is_active: c.is_active,
        created_at: c.created_at,
        updated_at: c.updated_at,
        client_secret: None,
    }
}

fn map_oauth_app_response(a: queries::oauth::OAuthAppData) -> OAuthAppResponse {
    let supported_scopes = a.supported_scopes_vec();
    let scope_definitions = a.scope_definitions_vec();
    OAuthAppResponse {
        id: a.id,
        slug: a.slug,
        name: a.name,
        description: a.description,
        logo_url: a.logo_url,
        fqdn: a.fqdn,
        supported_scopes,
        scope_definitions,
        allow_dynamic_client_registration: a.allow_dynamic_client_registration,
        is_active: a.is_active,
        created_at: a.created_at,
        updated_at: a.updated_at,
    }
}

async fn set_oauth_scope_archived(
    app_state: AppState,
    deployment_id: i64,
    params: OAuthScopePathParams,
    archived: bool,
) -> ApiResult<OAuthAppResponse> {
    let scope = params.scope.trim().to_string();
    if scope.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "scope is required").into());
    }

    let oauth_app = GetOAuthAppBySlugQuery::new(deployment_id, params.oauth_app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found"))?;

    let mut scope_definitions = oauth_app.scope_definitions_vec();
    let scope_definition = scope_definitions
        .iter_mut()
        .find(|definition| definition.scope == scope)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "scope definition not found"))?;
    scope_definition.archived = archived;

    let oauth_app_slug = oauth_app.slug.clone();
    let supported_scopes = oauth_app.supported_scopes_vec();
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
    .execute(&app_state)
    .await?;

    Ok(map_oauth_app_response(updated).into())
}
