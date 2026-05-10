use super::{api_key_context::OAuthMachineContext, deployment_context::DeploymentContext};
use crate::application::response::ApiErrorResponse;
use axum::{
    body::Body,
    extract::{Request, State},
    response::Response,
};
use common::{db_router::ReadConsistency, state::AppState};
use queries::deployment::GetDeploymentWithProjectQuery;
use wacht::gateway::{GatewayAuthzOptions, GatewayDenyReason, GatewayPrincipalType};
use wacht::middleware::auth::{AuthContext, TokenClaims};

pub async fn machine_deployment_middleware(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: axum::middleware::Next,
) -> Result<Response, ApiErrorResponse> {
    let access_token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let access_token = match access_token {
        Some(token) => token,
        None => {
            return Err(ApiErrorResponse::unauthorized(
                "Authorization header must be Bearer token",
            ));
        }
    };

    let wacht_client = state
        .wacht_client
        .clone()
        .expect("wacht_client must be configured for machine router");

    let method = req.method().as_str().to_string();
    let resource = req.uri().path().to_string();
    let required_permission = required_machine_permission(&method);
    let response = wacht_client
        .gateway()
        .check_authz_with_principal_type(
            GatewayPrincipalType::OauthAccessToken,
            access_token,
            &method,
            &resource,
            GatewayAuthzOptions {
                required_permissions: Some(vec![required_permission.to_string()]),
                ..GatewayAuthzOptions::default()
            },
        )
        .await
        .map_err(|_| ApiErrorResponse::unauthorized("Authentication failed"))?;

    if !response.allowed {
        return Err(
            if response.reason == Some(GatewayDenyReason::PermissionDenied) {
                ApiErrorResponse::forbidden("Permission denied for this resource")
            } else {
                ApiErrorResponse::new(
                    axum::http::StatusCode::TOO_MANY_REQUESTS,
                    format!(
                        "Rate limit exceeded. Retry after {} seconds",
                        response.retry_after.unwrap_or(60)
                    ),
                )
            },
        );
    }

    let auth_context = auth_context_from_oauth_response(&response)?;

    if let Some(path_deployment_id) = deployment_id_from_path(req.uri().path()) {
        validate_deployment_access(&state, path_deployment_id, &auth_context).await?;
        req.extensions_mut().insert(DeploymentContext {
            deployment_id: path_deployment_id,
        });
    }

    req.extensions_mut().insert(auth_context.clone());
    req.extensions_mut().insert(OAuthMachineContext {
        oauth_client_id: response.key_id,
        app_slug: response.app_slug,
        permissions: response.permissions,
        owner_user_id: response.owner_user_id,
        organization_id: auth_context.organization_id,
        workspace_id: auth_context.workspace_id,
    });

    Ok(next.run(req).await)
}

fn deployment_id_from_path(path: &str) -> Option<i64> {
    let rest = path.strip_prefix("/deployments/")?;
    rest.split('/').next()?.parse().ok()
}

fn required_machine_permission(method: &str) -> &'static str {
    match method {
        "GET" | "HEAD" | "OPTIONS" => "read",
        _ => "write",
    }
}

fn auth_context_from_oauth_response(
    response: &wacht::gateway::GatewayCheckResponse,
) -> Result<AuthContext, ApiErrorResponse> {
    let principal = response.resolve_principal_context();
    let granted_resource = principal.metadata.granted_resource.as_deref();

    let mut user_id = response.owner_user_id.map(|id| id.to_string());
    let mut organization_id = None;
    let mut workspace_id = None;

    let Some(resource) = granted_resource else {
        return Err(ApiErrorResponse::unauthorized(
            "OAuth token is missing granted_resource",
        ));
    };

    match parse_wacht_resource(resource) {
        Some(("user", id)) => user_id = Some(id.to_string()),
        Some(("organization", id)) => organization_id = Some(id.to_string()),
        Some(("workspace", id)) => workspace_id = Some(id.to_string()),
        _ => {
            return Err(ApiErrorResponse::unauthorized(
                "OAuth token has invalid granted_resource",
            ));
        }
    }

    let user_id = user_id.ok_or_else(|| {
        ApiErrorResponse::unauthorized("OAuth token is not associated with a Wacht user")
    })?;
    let session_id = format!("oauth:{}", response.key_id);

    Ok(AuthContext {
        user_id: user_id.clone(),
        session_id: session_id.clone(),
        organization_id: organization_id.clone(),
        workspace_id: workspace_id.clone(),
        permissions: None,
        claims: TokenClaims {
            iss: String::new(),
            sub: user_id,
            iat: 0,
            exp: i64::MAX,
            sid: session_id,
            organization: organization_id,
            workspace: workspace_id,
            permissions: None,
            claims: None,
            metadata: None,
            custom_claims: Default::default(),
        },
    })
}

fn parse_wacht_resource(resource: &str) -> Option<(&str, &str)> {
    let mut parts = resource.split(':');
    if parts.next()? != "urn" || parts.next()? != "wacht" {
        return None;
    }
    let resource_type = parts.next()?;
    let resource_id = parts.next()?;
    if resource_id.is_empty() {
        return None;
    }
    Some((resource_type, resource_id))
}

async fn validate_deployment_access(
    state: &AppState,
    deployment_id: i64,
    auth_context: &AuthContext,
) -> Result<(), ApiErrorResponse> {
    let deployment_with_project = GetDeploymentWithProjectQuery::new(deployment_id)
        .execute_with_db(state.db_router.reader(ReadConsistency::Strong))
        .await
        .map_err(|_| ApiErrorResponse::internal("Failed to verify access"))?;

    let deployment_with_project = deployment_with_project
        .ok_or_else(|| ApiErrorResponse::not_found("Deployment not found"))?;

    let has_access = match &deployment_with_project.project_owner_id {
        Some(owner_id) => {
            if auth_context.organization_id.is_some() {
                owner_id == &auth_context.user_id
                    || auth_context
                        .organization_id
                        .as_ref()
                        .map_or(false, |org_id| owner_id == org_id)
            } else {
                owner_id == &auth_context.user_id
            }
        }
        None => false,
    };

    if has_access {
        Ok(())
    } else {
        Err(ApiErrorResponse::forbidden(
            "You don't have permission to access this deployment",
        ))
    }
}
