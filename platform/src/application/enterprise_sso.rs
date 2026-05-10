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
    enterprise_connection::{EnterpriseConnection, EnterpriseConnectionProtocol},
    organization_domain::OrganizationDomain,
    scim_token::ScimToken,
};
use queries::{
    GetEnterpriseConnectionQuery, GetScimTokenQuery, ListEnterpriseConnectionsQuery,
    ListOrganizationDomainsQuery,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

use crate::application::{AppError, AppState};
use common::deps;

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
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn create_domain(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateOrganizationDomainRequest,
) -> Result<CreateOrganizationDomainResponse, AppError> {
    let domain_id = app_state.sf.next_id()? as i64;
    CreateOrganizationDomainCommand::builder()
        .domain_id(domain_id)
        .deployment_id(deployment_id)
        .request(request)
        .build()?
        .execute_with_db(app_state.db_router.writer())
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
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn verify_domain(
    app_state: &AppState,
    deployment_id: i64,
    request: VerifyOrganizationDomainRequest,
) -> Result<VerifyOrganizationDomainResponse, AppError> {
    let verify_deps = deps::from_app(app_state).db().dns();
    VerifyOrganizationDomainCommand::builder()
        .deployment_id(deployment_id)
        .request(request)
        .build()?
        .execute_with_deps(&verify_deps)
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
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn create_connection(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateEnterpriseConnectionRequest,
) -> Result<EnterpriseConnection, AppError> {
    let connection_id = app_state.sf.next_id()? as i64;
    CreateEnterpriseConnectionCommand::builder()
        .connection_id(connection_id)
        .deployment_id(deployment_id)
        .request(request)
        .build()?
        .execute_with_db(app_state.db_router.writer())
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
        .execute_with_db(app_state.db_router.writer())
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
        .execute_with_db(app_state.db_router.writer())
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
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn generate_scim_token(
    app_state: &AppState,
    deployment_id: i64,
    request: GenerateScimTokenRequest,
) -> Result<GenerateScimTokenResponse, AppError> {
    let token_id = app_state.sf.next_id()? as i64;
    GenerateScimTokenCommand::builder()
        .token_id(token_id)
        .deployment_id(deployment_id)
        .request(request)
        .build()?
        .execute_with_db(app_state.db_router.writer())
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
        .execute_with_db(app_state.db_router.writer())
        .await
}

#[derive(Debug, Deserialize)]
pub struct TestEnterpriseConnectionConfig {
    pub protocol: EnterpriseConnectionProtocol,
    pub idp_certificate: Option<String>,
    pub idp_sso_url: Option<String>,
    pub oidc_issuer_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TestConnectionResult {
    pub success: bool,
    pub protocol: EnterpriseConnectionProtocol,
    pub checks: HashMap<String, bool>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub errors: HashMap<String, String>,
}

pub async fn test_connection_config(input: TestEnterpriseConnectionConfig) -> TestConnectionResult {
    let mut checks: HashMap<String, bool> = HashMap::new();
    let mut errors: HashMap<String, String> = HashMap::new();

    match input.protocol {
        EnterpriseConnectionProtocol::Saml => {
            validate_saml(
                input.idp_certificate.as_deref().unwrap_or(""),
                input.idp_sso_url.as_deref().unwrap_or(""),
                &mut checks,
                &mut errors,
            )
            .await;
        }
        EnterpriseConnectionProtocol::Oidc => {
            validate_oidc(
                input.oidc_issuer_url.as_deref().unwrap_or(""),
                &mut checks,
                &mut errors,
            )
            .await;
        }
    }

    TestConnectionResult {
        success: errors.is_empty(),
        protocol: input.protocol,
        checks,
        errors,
    }
}

pub async fn test_existing_connection(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
    connection_id: i64,
) -> Result<TestConnectionResult, AppError> {
    let connection = GetEnterpriseConnectionQuery::builder()
        .deployment_id(deployment_id)
        .organization_id(organization_id)
        .connection_id(connection_id)
        .build()?
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?
        .ok_or_else(|| AppError::NotFound("Enterprise connection not found".to_string()))?;

    Ok(test_connection_config(TestEnterpriseConnectionConfig {
        protocol: connection.protocol,
        idp_certificate: connection.idp_certificate,
        idp_sso_url: connection.idp_sso_url,
        oidc_issuer_url: connection.oidc_issuer_url,
    })
    .await)
}

async fn validate_saml(
    certificate: &str,
    sso_url: &str,
    checks: &mut HashMap<String, bool>,
    errors: &mut HashMap<String, String>,
) {
    if certificate.is_empty() {
        checks.insert("certificate_valid".to_string(), false);
        errors.insert(
            "certificate_valid".to_string(),
            "Certificate is required".to_string(),
        );
    } else {
        let body = if certificate.contains("BEGIN CERTIFICATE") {
            extract_pem_body(certificate)
        } else {
            certificate.split_whitespace().collect::<String>()
        };
        match STANDARD.decode(body) {
            Ok(_) => {
                checks.insert("certificate_valid".to_string(), true);
            }
            Err(e) => {
                checks.insert("certificate_valid".to_string(), false);
                errors.insert(
                    "certificate_valid".to_string(),
                    format!("Certificate is not valid PEM/base64: {}", e),
                );
            }
        }
    }

    if sso_url.is_empty() {
        checks.insert("sso_url_reachable".to_string(), false);
        errors.insert(
            "sso_url_reachable".to_string(),
            "SSO URL is required".to_string(),
        );
        return;
    }

    if sso_url.contains('{') || sso_url.contains('}') {
        checks.insert("sso_url_reachable".to_string(), false);
        errors.insert(
            "sso_url_reachable".to_string(),
            "SSO URL contains placeholder values (e.g., {appId}) - please replace with actual values"
                .to_string(),
        );
        return;
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            checks.insert("sso_url_reachable".to_string(), false);
            errors.insert(
                "sso_url_reachable".to_string(),
                format!("HTTP client init failed: {}", e),
            );
            return;
        }
    };

    match client.get(sso_url).send().await {
        Ok(resp) => {
            let ok = resp.status().as_u16() < 500;
            checks.insert("sso_url_reachable".to_string(), ok);
            if !ok {
                errors.insert(
                    "sso_url_reachable".to_string(),
                    format!("SSO URL returned server error: {}", resp.status().as_u16()),
                );
            }
        }
        Err(e) => {
            checks.insert("sso_url_reachable".to_string(), false);
            errors.insert(
                "sso_url_reachable".to_string(),
                format!("SSO URL is not reachable: {}", e),
            );
        }
    }
}

fn extract_pem_body(pem: &str) -> String {
    pem.lines()
        .filter(|line| !line.starts_with("-----"))
        .collect::<String>()
        .split_whitespace()
        .collect()
}

async fn validate_oidc(
    issuer_url: &str,
    checks: &mut HashMap<String, bool>,
    errors: &mut HashMap<String, String>,
) {
    if issuer_url.is_empty() {
        checks.insert("issuer_valid".to_string(), false);
        errors.insert(
            "issuer_valid".to_string(),
            "Issuer URL is required".to_string(),
        );
        return;
    }

    let discovery_url = format!(
        "{}/.well-known/openid-configuration",
        issuer_url.trim_end_matches('/')
    );
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            checks.insert("discovery_reachable".to_string(), false);
            errors.insert(
                "discovery_reachable".to_string(),
                format!("HTTP client init failed: {}", e),
            );
            return;
        }
    };

    let resp = match client.get(&discovery_url).send().await {
        Ok(r) => r,
        Err(e) => {
            checks.insert("discovery_reachable".to_string(), false);
            errors.insert(
                "discovery_reachable".to_string(),
                format!("Failed to fetch OIDC discovery document: {}", e),
            );
            return;
        }
    };

    if resp.status().as_u16() != 200 {
        checks.insert("discovery_reachable".to_string(), false);
        errors.insert(
            "discovery_reachable".to_string(),
            format!("OIDC discovery returned status {}", resp.status().as_u16()),
        );
        return;
    }

    checks.insert("discovery_reachable".to_string(), true);

    let body = match resp.bytes().await {
        Ok(b) => b,
        Err(_) => {
            checks.insert("discovery_valid".to_string(), false);
            errors.insert(
                "discovery_valid".to_string(),
                "Failed to read discovery document".to_string(),
            );
            return;
        }
    };

    let discovery: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            checks.insert("discovery_valid".to_string(), false);
            errors.insert(
                "discovery_valid".to_string(),
                "Invalid JSON in discovery document".to_string(),
            );
            return;
        }
    };

    let auth_present = discovery.get("authorization_endpoint").is_some();
    checks.insert("authorization_endpoint".to_string(), auth_present);
    if !auth_present {
        errors.insert(
            "authorization_endpoint".to_string(),
            "Missing authorization_endpoint".to_string(),
        );
    }

    let token_present = discovery.get("token_endpoint").is_some();
    checks.insert("token_endpoint".to_string(), token_present);
    if !token_present {
        errors.insert(
            "token_endpoint".to_string(),
            "Missing token_endpoint".to_string(),
        );
    }

    checks.insert("discovery_valid".to_string(), true);
}
