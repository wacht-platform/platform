use commands::{
    Command, UploadToCdnCommand,
    oauth::{CreateOAuthAppCommand, UpdateOAuthAppCommand, VerifyOAuthAppDomainCommand},
};
use common::db_router::ReadConsistency;
use common::state::AppState;
use dto::json::api_key::{
    ListOAuthAppsResponse, OAuthAppResponse, UpdateOAuthAppRequest, VerifyOAuthAppDomainResponse,
};
use models::api_key::OAuthScopeDefinition;
use models::error::AppError;
use queries::oauth::ListOAuthAppsByDeploymentQuery;

use super::oauth_shared::map_oauth_app_response;

pub struct CreateOAuthAppInput {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub fqdn: Option<String>,
    pub supported_scopes: Vec<String>,
    pub scope_definitions: Option<Vec<OAuthScopeDefinition>>,
    pub allow_dynamic_client_registration: bool,
    pub logo_image_data: Option<(Vec<u8>, String)>,
}

pub async fn verify_oauth_app_domain(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
) -> Result<VerifyOAuthAppDomainResponse, AppError> {
    let writer = app_state.db_router.writer();
    let result = VerifyOAuthAppDomainCommand {
        deployment_id,
        oauth_app_slug,
    }
    .execute_with(writer, &app_state.cloudflare_service)
    .await?;

    Ok(VerifyOAuthAppDomainResponse {
        domain: result.domain,
        cname_target: result.cname_target,
        verified: result.verified,
    })
}

pub async fn list_oauth_apps(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<ListOAuthAppsResponse, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let apps = ListOAuthAppsByDeploymentQuery::new(deployment_id)
        .execute_with(reader)
        .await?
        .into_iter()
        .map(map_oauth_app_response)
        .collect();

    Ok(ListOAuthAppsResponse { apps })
}

pub async fn create_oauth_app(
    app_state: &AppState,
    deployment_id: i64,
    input: CreateOAuthAppInput,
) -> Result<OAuthAppResponse, AppError> {
    let writer = app_state.db_router.writer();
    let logo_url = if let Some((image_buffer, file_extension)) = input.logo_image_data {
        let file_path = format!(
            "deployments/{}/oauth-apps/{}/logo.{}",
            deployment_id, input.slug, file_extension
        );
        Some(
            UploadToCdnCommand::new(file_path, image_buffer)
                .execute(app_state)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?,
        )
    } else {
        None
    };

    let created = CreateOAuthAppCommand {
        deployment_id,
        slug: input.slug,
        name: input.name,
        description: input.description,
        logo_url,
        fqdn: input.fqdn,
        supported_scopes: input.supported_scopes,
        scope_definitions: input.scope_definitions,
        allow_dynamic_client_registration: input.allow_dynamic_client_registration,
    }
    .execute_with(
        writer,
        &app_state.cloudflare_service,
        app_state.sf.next_id()? as i64,
    )
    .await?;

    Ok(map_oauth_app_response(created))
}

pub async fn update_oauth_app(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
    request: UpdateOAuthAppRequest,
) -> Result<OAuthAppResponse, AppError> {
    let writer = app_state.db_router.writer();
    let updated = UpdateOAuthAppCommand {
        deployment_id,
        oauth_app_slug,
        name: request.name,
        description: request.description,
        supported_scopes: request.supported_scopes,
        scope_definitions: request.scope_definitions,
        allow_dynamic_client_registration: request.allow_dynamic_client_registration,
        is_active: request.is_active,
    }
    .execute_with(writer)
    .await?;

    Ok(map_oauth_app_response(updated))
}
