use axum::Json;
use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use models::api_key::OAuthScopeDefinition;

use crate::api::multipart::{MultipartField, MultipartPayload};
use crate::application::response::{ApiErrorResponse, ApiResult};
use crate::middleware::RequireDeployment;
use commands::{
    Command, UploadToCdnCommand,
    oauth::{CreateOAuthAppCommand, UpdateOAuthAppCommand, VerifyOAuthAppDomainCommand},
};
use common::state::AppState;
use dto::json::api_key::{
    ListOAuthAppsResponse, OAuthAppResponse, UpdateOAuthAppRequest, VerifyOAuthAppDomainResponse,
};
use queries::{Query as QueryTrait, oauth::ListOAuthAppsByDeploymentQuery};

use super::types::OAuthAppPathParams;

fn parse_logo_upload(field: &MultipartField) -> Result<Option<(Vec<u8>, String)>, ApiErrorResponse> {
    let Some(file_extension) = field.image_extension()? else {
        return Ok(None);
    };

    if field.bytes.is_empty() {
        return Ok(None);
    }

    Ok(Some((field.bytes.clone(), file_extension.to_string())))
}

pub(crate) async fn verify_oauth_app_domain(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthAppPathParams>,
) -> ApiResult<VerifyOAuthAppDomainResponse> {
    let result = VerifyOAuthAppDomainCommand {
        deployment_id,
        oauth_app_slug: params.oauth_app_slug,
    }
    .execute(&app_state)
    .await?;

    Ok(VerifyOAuthAppDomainResponse {
        domain: result.domain,
        cname_target: result.cname_target,
        verified: result.verified,
    }
    .into())
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
    multipart: Multipart,
) -> ApiResult<OAuthAppResponse> {
    let mut slug: Option<String> = None;
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut fqdn: Option<String> = None;
    let mut supported_scopes: Vec<String> = vec![];
    let mut scope_definitions: Option<Vec<OAuthScopeDefinition>> = None;
    let mut allow_dynamic_client_registration = false;
    let mut logo_image_data: Option<(Vec<u8>, String)> = None;
    let payload = MultipartPayload::parse(multipart).await?;

    for field in payload.fields() {
        match field.name.as_str() {
            "slug" => {
                let trimmed = field.text_trimmed()?;
                if !trimmed.is_empty() {
                    slug = Some(trimmed);
                }
            }
            "name" => {
                let trimmed = field.text_trimmed()?;
                if !trimmed.is_empty() {
                    name = Some(trimmed);
                }
            }
            "description" => {
                let trimmed = field.text_trimmed()?;
                if !trimmed.is_empty() {
                    description = Some(trimmed);
                }
            }
            "fqdn" | "domain" => {
                let trimmed = field.text_trimmed()?;
                if !trimmed.is_empty() {
                    fqdn = Some(trimmed);
                }
            }
            "supported_scopes" => {
                let value = field.text()?;
                supported_scopes = value
                    .split(',')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
            }
            "scope_definitions" => {
                let value = field.text()?;
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
                let value = field.text()?;
                allow_dynamic_client_registration = matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                );
            }
            "logo" => {
                if let Some(image) = parse_logo_upload(field)? {
                    logo_image_data = Some(image);
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
