use super::api_key_context::ApiKeyContext;
use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::Response,
};
use common::state::AppState;
use wacht::gateway::{GatewayDenyReason, GatewayPrincipalType};

/// Deployment context that gets injected into request extensions
#[derive(Clone, Copy, Debug)]
pub struct DeploymentContext {
    pub deployment_id: i64,
}

pub async fn backend_deployment_middleware(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: axum::middleware::Next,
) -> Result<Response, (StatusCode, String)> {
    let api_key = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or(Some(v)));

    let api_key = match api_key {
        Some(key) => key,
        None => {
            return Err((StatusCode::UNAUTHORIZED, "API key required".to_string()));
        }
    };

    let wacht_client = state
        .wacht_client
        .clone()
        .expect("wacht_client must be configured for backend router");

    let method = req.method().as_str().to_string();
    let resource = req.uri().path().to_string();

    let mut authz_response = None;
    let mut last_error = None;

    for principal_type in [
        GatewayPrincipalType::ApiKey,
        GatewayPrincipalType::OauthAccessToken,
    ] {
        match wacht_client
            .gateway()
            .verify_request_with_principal_type(principal_type, api_key, &method, &resource)
            .await
        {
            Ok(response) => {
                authz_response = Some(response);
                break;
            }
            Err(e) => {
                last_error = Some(e.to_string());
            }
        }
    }

    let response = authz_response.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            format!(
                "Authentication failed: {}",
                last_error.unwrap_or_else(|| "invalid token".to_string())
            ),
        )
    })?;

    if !response.allowed {
        return Err(
            if response.reason == Some(GatewayDenyReason::PermissionDenied) {
                (
                    StatusCode::FORBIDDEN,
                    "Permission denied for this resource".to_string(),
                )
            } else {
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    format!(
                        "Rate limit exceeded. Retry after {} seconds",
                        response.retry_after.unwrap_or(60)
                    ),
                )
            },
        );
    }

    req.extensions_mut().insert(DeploymentContext {
        deployment_id: response.deployment_id,
    });
    req.extensions_mut().insert(ApiKeyContext {
        key_id: response.key_id,
        app_slug: response.app_slug,
        permissions: response.permissions,
        organization_id: response.organization_id,
        workspace_id: response.workspace_id,
        organization_membership_id: response.organization_membership_id,
        workspace_membership_id: response.workspace_membership_id,
    });

    Ok(next.run(req).await)
}
