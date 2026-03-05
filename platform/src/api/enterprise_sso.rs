use axum::{
    Json,
    extract::{Path, Query, State},
};
use commands::{
    Command,
    enterprise_connection::{
        CreateEnterpriseConnectionCommand, CreateEnterpriseConnectionRequest,
        DeleteEnterpriseConnectionCommand, DeleteEnterpriseConnectionRequest,
        UpdateEnterpriseConnectionCommand, UpdateEnterpriseConnectionRequest,
    },
    organization_domain::{
        CreateOrganizationDomainCommand, CreateOrganizationDomainRequest,
        CreateOrganizationDomainResponse, DeleteOrganizationDomainCommand,
        DeleteOrganizationDomainRequest, VerifyOrganizationDomainCommand,
        VerifyOrganizationDomainRequest, VerifyOrganizationDomainResponse,
    },
};
use common::state::AppState;
use models::{
    enterprise_connection::EnterpriseConnection, organization_domain::OrganizationDomain,
    scim_token::ScimToken,
};
use queries::{ListEnterpriseConnectionsQuery, ListOrganizationDomainsQuery, Query as QueryTrait};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::api::pagination::paginate_results;
use crate::application::response::{ApiResult, PaginatedResponse};
use crate::middleware::RequireDeployment;

const DEFAULT_FRONTEND_API_URL: &str = "https://api.wacht.dev";

fn resolve_list_pagination(params: &ListQueryParams) -> (i32, i64) {
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);
    (limit, offset)
}

fn scim_base_url(connection_id: i64) -> String {
    let base =
        std::env::var("FRONTEND_API_URL").unwrap_or_else(|_| DEFAULT_FRONTEND_API_URL.to_string());
    format!("{}/scim/v2/{}", base, connection_id)
}

fn build_scim_token_details(token: &ScimToken, plain_token: Option<String>) -> ScimTokenDetails {
    ScimTokenDetails {
        token: plain_token,
        token_prefix: token.token_prefix.clone(),
        enabled: token.enabled,
        created_at: token.created_at.to_rfc3339(),
        updated_at: token.updated_at.to_rfc3339(),
        last_used_at: token.last_used_at.map(|dt| dt.to_rfc3339()),
    }
}

fn build_scim_token_response(
    connection_id: i64,
    token: Option<ScimTokenDetails>,
) -> ScimTokenInfoResponse {
    ScimTokenInfoResponse {
        exists: token.is_some(),
        scim_base_url: scim_base_url(connection_id),
        token,
    }
}

#[derive(Deserialize)]
pub struct OrganizationParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub org_id: i64,
}

#[derive(Deserialize)]
pub struct DomainParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub org_id: i64,
    pub domain_id: i64,
}

#[derive(Deserialize)]
pub struct ConnectionParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub org_id: i64,
    pub connection_id: i64,
}

#[derive(Deserialize, Default)]
pub struct ListQueryParams {
    pub limit: Option<i32>,
    pub offset: Option<i64>,
}

// --- Domain Handlers ---

pub async fn list_domains_handler(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    Query(query_params): Query<ListQueryParams>,
) -> ApiResult<PaginatedResponse<OrganizationDomain>> {
    let (limit, offset) = resolve_list_pagination(&query_params);

    let domains = ListOrganizationDomainsQuery::new(deployment_id, params.org_id)
        .limit(limit + 1)
        .offset(offset)
        .execute(&app_state)
        .await?;

    Ok(paginate_results(domains, limit, Some(offset)).into())
}

pub async fn create_domain_handler(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    Json(mut req): Json<CreateOrganizationDomainRequest>,
) -> ApiResult<CreateOrganizationDomainResponse> {
    req.organization_id = params.org_id;
    let created = CreateOrganizationDomainCommand::new(deployment_id, req)
        .execute(&app_state)
        .await?;
    Ok(created.into())
}

pub async fn delete_domain_handler(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<DomainParams>,
) -> ApiResult<()> {
    let req = DeleteOrganizationDomainRequest {
        organization_id: params.org_id,
        domain_id: params.domain_id,
    };
    DeleteOrganizationDomainCommand::new(deployment_id, req)
        .execute(&app_state)
        .await?;
    Ok(().into())
}

pub async fn verify_domain_handler(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<DomainParams>,
) -> ApiResult<VerifyOrganizationDomainResponse> {
    let req = VerifyOrganizationDomainRequest {
        organization_id: params.org_id,
        domain_id: params.domain_id,
    };
    let verified = VerifyOrganizationDomainCommand::new(deployment_id, req)
        .execute(&app_state)
        .await?;
    Ok(verified.into())
}

// --- Connection Handlers ---

pub async fn list_connections_handler(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    Query(query_params): Query<ListQueryParams>,
) -> ApiResult<PaginatedResponse<EnterpriseConnection>> {
    let (limit, offset) = resolve_list_pagination(&query_params);

    let connections = ListEnterpriseConnectionsQuery::new(deployment_id, params.org_id)
        .limit(limit + 1)
        .offset(offset)
        .execute(&app_state)
        .await?;

    Ok(paginate_results(connections, limit, Some(offset)).into())
}

pub async fn create_connection_handler(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    Json(mut req): Json<CreateEnterpriseConnectionRequest>,
) -> ApiResult<EnterpriseConnection> {
    req.organization_id = params.org_id;
    let connection = CreateEnterpriseConnectionCommand::new(deployment_id, req)
        .execute(&app_state)
        .await?;
    Ok(connection.into())
}

pub async fn update_connection_handler(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ConnectionParams>,
    Json(mut req): Json<UpdateEnterpriseConnectionRequest>,
) -> ApiResult<EnterpriseConnection> {
    req.organization_id = params.org_id;
    req.connection_id = params.connection_id;
    let connection = UpdateEnterpriseConnectionCommand::new(deployment_id, req)
        .execute(&app_state)
        .await?;
    Ok(connection.into())
}

pub async fn delete_connection_handler(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ConnectionParams>,
) -> ApiResult<()> {
    let req = DeleteEnterpriseConnectionRequest {
        organization_id: params.org_id,
        connection_id: params.connection_id,
    };
    DeleteEnterpriseConnectionCommand::new(deployment_id, req)
        .execute(&app_state)
        .await?;
    Ok(().into())
}

#[derive(Serialize)]
pub struct ScimTokenInfoResponse {
    pub exists: bool,
    pub scim_base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<ScimTokenDetails>,
}

#[derive(Serialize)]
pub struct ScimTokenDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    pub token_prefix: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
}

pub async fn get_scim_token_handler(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ConnectionParams>,
) -> ApiResult<ScimTokenInfoResponse> {
    let token = queries::GetScimTokenQuery::new(deployment_id, params.org_id, params.connection_id)
        .execute(&app_state)
        .await?;

    let response = build_scim_token_response(
        params.connection_id,
        token.as_ref().map(|scim_token| build_scim_token_details(scim_token, None)),
    );

    Ok(response.into())
}

pub async fn generate_scim_token_handler(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ConnectionParams>,
) -> ApiResult<ScimTokenInfoResponse> {
    use commands::scim_token::{GenerateScimTokenCommand, GenerateScimTokenRequest};

    let req = GenerateScimTokenRequest {
        organization_id: params.org_id,
        connection_id: params.connection_id,
    };

    let result = GenerateScimTokenCommand::new(deployment_id, req)
        .execute(&app_state)
        .await?;

    let token = build_scim_token_details(&result.token, Some(result.plain_token));
    let response = build_scim_token_response(params.connection_id, Some(token));

    Ok(response.into())
}

pub async fn revoke_scim_token_handler(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ConnectionParams>,
) -> ApiResult<()> {
    use commands::scim_token::{RevokeScimTokenCommand, RevokeScimTokenRequest};

    let req = RevokeScimTokenRequest {
        organization_id: params.org_id,
        connection_id: params.connection_id,
    };

    RevokeScimTokenCommand::new(deployment_id, req)
        .execute(&app_state)
        .await?;
    Ok(().into())
}
