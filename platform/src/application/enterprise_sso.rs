use commands::{
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
    scim_token::{
        GenerateScimTokenCommand, GenerateScimTokenRequest, GenerateScimTokenResponse,
        RevokeScimTokenCommand, RevokeScimTokenRequest,
    },
};
use common::db_router::ReadConsistency;
use models::{
    enterprise_connection::EnterpriseConnection, organization_domain::OrganizationDomain,
    scim_token::ScimToken,
};
use queries::{GetScimTokenQuery, ListEnterpriseConnectionsQuery, ListOrganizationDomainsQuery};

use crate::application::{AppError, AppState};

pub struct ListInput {
    deployment_id: i64,
    organization_id: i64,
    limit: i32,
    offset: i64,
}

impl ListInput {
    pub fn new(deployment_id: i64, organization_id: i64, limit: i32, offset: i64) -> Self {
        Self {
            deployment_id,
            organization_id,
            limit,
            offset,
        }
    }
}

pub async fn list_domains(
    app_state: &AppState,
    input: ListInput,
) -> Result<Vec<OrganizationDomain>, AppError> {
    ListOrganizationDomainsQuery::builder()
        .deployment_id(input.deployment_id)
        .organization_id(input.organization_id)
        .limit(input.limit)
        .offset(input.offset)
        .build()?
        .execute_with(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn create_domain(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateOrganizationDomainRequest,
) -> Result<CreateOrganizationDomainResponse, AppError> {
    let domain_id = app_state.sf.next_id()? as i64;
    CreateOrganizationDomainCommand::builder()
        .deployment_id(deployment_id)
        .request(request)
        .build()?
        .execute_with(app_state.db_router.writer(), domain_id)
        .await
}

pub async fn delete_domain(
    app_state: &AppState,
    deployment_id: i64,
    request: DeleteOrganizationDomainRequest,
) -> Result<(), AppError> {
    DeleteOrganizationDomainCommand::builder()
        .deployment_id(deployment_id)
        .request(request)
        .build()?
        .execute_with(app_state.db_router.writer())
        .await
}

pub async fn verify_domain(
    app_state: &AppState,
    deployment_id: i64,
    request: VerifyOrganizationDomainRequest,
) -> Result<VerifyOrganizationDomainResponse, AppError> {
    VerifyOrganizationDomainCommand::builder()
        .deployment_id(deployment_id)
        .request(request)
        .build()?
        .execute_with(
            app_state.db_router.writer(),
            &app_state.dns_verification_service,
        )
        .await
}

pub async fn list_connections(
    app_state: &AppState,
    input: ListInput,
) -> Result<Vec<EnterpriseConnection>, AppError> {
    ListEnterpriseConnectionsQuery::builder()
        .deployment_id(input.deployment_id)
        .organization_id(input.organization_id)
        .limit(input.limit)
        .offset(input.offset)
        .build()?
        .execute_with(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn create_connection(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateEnterpriseConnectionRequest,
) -> Result<EnterpriseConnection, AppError> {
    let connection_id = app_state.sf.next_id()? as i64;
    CreateEnterpriseConnectionCommand::builder()
        .deployment_id(deployment_id)
        .request(request)
        .build()?
        .execute_with(app_state.db_router.writer(), connection_id)
        .await
}

pub async fn update_connection(
    app_state: &AppState,
    deployment_id: i64,
    request: UpdateEnterpriseConnectionRequest,
) -> Result<EnterpriseConnection, AppError> {
    UpdateEnterpriseConnectionCommand::builder()
        .deployment_id(deployment_id)
        .request(request)
        .build()?
        .execute_with(app_state.db_router.writer())
        .await
}

pub async fn delete_connection(
    app_state: &AppState,
    deployment_id: i64,
    request: DeleteEnterpriseConnectionRequest,
) -> Result<(), AppError> {
    DeleteEnterpriseConnectionCommand::builder()
        .deployment_id(deployment_id)
        .request(request)
        .build()?
        .execute_with(app_state.db_router.writer())
        .await
}

pub async fn get_scim_token(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
    connection_id: i64,
) -> Result<Option<ScimToken>, AppError> {
    GetScimTokenQuery::builder()
        .deployment_id(deployment_id)
        .organization_id(organization_id)
        .connection_id(connection_id)
        .build()?
        .execute_with(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn generate_scim_token(
    app_state: &AppState,
    deployment_id: i64,
    request: GenerateScimTokenRequest,
) -> Result<GenerateScimTokenResponse, AppError> {
    let token_id = app_state.sf.next_id()? as i64;
    GenerateScimTokenCommand::builder()
        .deployment_id(deployment_id)
        .request(request)
        .build()?
        .execute_with(app_state.db_router.writer(), token_id)
        .await
}

pub async fn revoke_scim_token(
    app_state: &AppState,
    deployment_id: i64,
    request: RevokeScimTokenRequest,
) -> Result<(), AppError> {
    RevokeScimTokenCommand::builder()
        .deployment_id(deployment_id)
        .request(request)
        .build()?
        .execute_with(app_state.db_router.writer())
        .await
}
