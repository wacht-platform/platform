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
};
use queries::{ListEnterpriseConnectionsQuery, ListOrganizationDomainsQuery, Query as QueryTrait};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::application::response::{ApiResult, PaginatedResponse};
use crate::middleware::RequireDeployment;

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
    let limit = query_params.limit.unwrap_or(50);

    let domains = ListOrganizationDomainsQuery::new(deployment_id, params.org_id)
        .limit(limit + 1)
        .offset(query_params.offset.unwrap_or(0))
        .execute(&app_state)
        .await?;

    let has_more = domains.len() > limit as usize;
    let domains = if has_more {
        domains[..limit as usize].to_vec()
    } else {
        domains
    };

    Ok(PaginatedResponse {
        data: domains,
        has_more,
        limit: Some(limit),
        offset: Some(query_params.offset.unwrap_or(0) as i32),
    }
    .into())
}

pub async fn create_domain_handler(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    Json(mut req): Json<CreateOrganizationDomainRequest>,
) -> ApiResult<CreateOrganizationDomainResponse> {
    req.organization_id = params.org_id;
    CreateOrganizationDomainCommand::new(deployment_id, req)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
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
        .await
        .map(Into::into)
        .map_err(Into::into)
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
    VerifyOrganizationDomainCommand::new(deployment_id, req)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

// --- Connection Handlers ---

pub async fn list_connections_handler(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    Query(query_params): Query<ListQueryParams>,
) -> ApiResult<PaginatedResponse<EnterpriseConnection>> {
    let limit = query_params.limit.unwrap_or(50);

    let connections = ListEnterpriseConnectionsQuery::new(deployment_id, params.org_id)
        .limit(limit + 1)
        .offset(query_params.offset.unwrap_or(0))
        .execute(&app_state)
        .await?;

    let has_more = connections.len() > limit as usize;
    let connections = if has_more {
        connections[..limit as usize].to_vec()
    } else {
        connections
    };

    Ok(PaginatedResponse {
        data: connections,
        has_more,
        limit: Some(limit),
        offset: Some(query_params.offset.unwrap_or(0) as i32),
    }
    .into())
}

pub async fn create_connection_handler(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    Json(mut req): Json<CreateEnterpriseConnectionRequest>,
) -> ApiResult<EnterpriseConnection> {
    req.organization_id = params.org_id;
    CreateEnterpriseConnectionCommand::new(deployment_id, req)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_connection_handler(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ConnectionParams>,
    Json(mut req): Json<UpdateEnterpriseConnectionRequest>,
) -> ApiResult<EnterpriseConnection> {
    req.organization_id = params.org_id;
    req.connection_id = params.connection_id;
    UpdateEnterpriseConnectionCommand::new(deployment_id, req)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
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
        .await
        .map(Into::into)
        .map_err(Into::into)
}

// --- SCIM Token Handlers ---

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
    let scim_base_url = format!(
        "{}/scim/v2/{}",
        std::env::var("FRONTEND_API_URL").unwrap_or_else(|_| "https://api.wacht.dev".to_string()),
        params.connection_id
    );

    let token = queries::GetScimTokenQuery::new(deployment_id, params.org_id, params.connection_id)
        .execute(&app_state)
        .await?;

    let response = match token {
        Some(t) => ScimTokenInfoResponse {
            exists: true,
            scim_base_url,
            token: Some(ScimTokenDetails {
                token: None,
                token_prefix: t.token_prefix,
                enabled: t.enabled,
                created_at: t.created_at.to_rfc3339(),
                updated_at: t.updated_at.to_rfc3339(),
                last_used_at: t.last_used_at.map(|dt| dt.to_rfc3339()),
            }),
        },
        None => ScimTokenInfoResponse {
            exists: false,
            scim_base_url,
            token: None,
        },
    };

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

    let scim_base_url = format!(
        "{}/scim/v2/{}",
        std::env::var("FRONTEND_API_URL").unwrap_or_else(|_| "https://api.wacht.dev".to_string()),
        params.connection_id
    );

    let response = ScimTokenInfoResponse {
        exists: true,
        scim_base_url,
        token: Some(ScimTokenDetails {
            token: Some(result.plain_token),
            token_prefix: result.token.token_prefix,
            enabled: result.token.enabled,
            created_at: result.token.created_at.to_rfc3339(),
            updated_at: result.token.updated_at.to_rfc3339(),
            last_used_at: None,
        }),
    };

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
        .await
        .map(Into::into)
        .map_err(Into::into)
}
