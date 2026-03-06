use axum::Json;
use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use models::api_key::OAuthScopeDefinition;

use crate::api::multipart::MultipartPayload;
use crate::application::{oauth_app as oauth_app_use_cases, response::ApiResult};
use crate::middleware::RequireDeployment;
use common::state::AppState;
use dto::json::api_key::{
    ListOAuthAppsResponse, OAuthAppResponse, UpdateOAuthAppRequest, VerifyOAuthAppDomainResponse,
};

use super::types::OAuthAppPathParams;

async fn parse_create_oauth_app_input(
    multipart: Multipart,
) -> Result<oauth_app_use_cases::CreateOAuthAppInput, crate::application::response::ApiErrorResponse> {
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
            "slug" => slug = field.optional_text_trimmed()?,
            "name" => name = field.optional_text_trimmed()?,
            "description" => description = field.optional_text_trimmed()?,
            "fqdn" | "domain" => fqdn = field.optional_text_trimmed()?,
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
                if let Some(image) = field.image_upload()? {
                    logo_image_data = Some(image);
                }
            }
            _ => {}
        }
    }

    let slug = slug.ok_or((StatusCode::BAD_REQUEST, "slug is required"))?;
    let name = name.ok_or((StatusCode::BAD_REQUEST, "name is required"))?;

    Ok(oauth_app_use_cases::CreateOAuthAppInput {
        slug,
        name,
        description,
        fqdn,
        supported_scopes,
        scope_definitions,
        allow_dynamic_client_registration,
        logo_image_data,
    })
}

pub(crate) async fn verify_oauth_app_domain(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthAppPathParams>,
) -> ApiResult<VerifyOAuthAppDomainResponse> {
    let result = oauth_app_use_cases::verify_oauth_app_domain(
        &app_state,
        deployment_id,
        params.oauth_app_slug,
    )
    .await?;
    Ok(result.into())
}

pub async fn list_oauth_apps(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ListOAuthAppsResponse> {
    let apps = oauth_app_use_cases::list_oauth_apps(&app_state, deployment_id).await?;
    Ok(apps.into())
}

pub async fn create_oauth_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    multipart: Multipart,
) -> ApiResult<OAuthAppResponse> {
    let input = parse_create_oauth_app_input(multipart).await?;
    let app = oauth_app_use_cases::create_oauth_app(&app_state, deployment_id, input).await?;
    Ok(app.into())
}

pub(crate) async fn update_oauth_app(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthAppPathParams>,
    Json(request): Json<UpdateOAuthAppRequest>,
) -> ApiResult<OAuthAppResponse> {
    let app = oauth_app_use_cases::update_oauth_app(
        &app_state,
        deployment_id,
        params.oauth_app_slug,
        request,
    )
    .await?;
    Ok(app.into())
}
