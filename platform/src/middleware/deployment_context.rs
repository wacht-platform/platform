use super::api_key_context::ApiKeyContext;
use crate::application::response::ApiErrorResponse;
use axum::{
    body::Body,
    extract::{Request, State},
    response::Response,
};
use common::state::AppState;
use wacht::gateway::{GatewayAuthzOptions, GatewayDenyReason, GatewayPrincipalType};

/// Deployment context that gets injected into request extensions
#[derive(Clone, Copy, Debug)]
pub struct DeploymentContext {
    pub deployment_id: i64,
}

pub async fn backend_deployment_middleware(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: axum::middleware::Next,
) -> Result<Response, ApiErrorResponse> {
    let api_key = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let api_key = match api_key {
        Some(key) => key,
        None => {
            return Err(ApiErrorResponse::unauthorized(
                "Authorization header must be Bearer token",
            ));
        }
    };

    let wacht_client = state
        .wacht_client
        .clone()
        .expect("wacht_client must be configured for backend router");

    let method = req.method().as_str().to_string();
    let resource = req.uri().path().to_string();
    let response = wacht_client
        .gateway()
        .check_authz_with_principal_type(
            GatewayPrincipalType::ApiKey,
            api_key,
            &method,
            &resource,
            GatewayAuthzOptions::default(),
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

    let deployment_id = response
        .app_slug
        .strip_prefix("aa_")
        .and_then(|raw| raw.parse::<i64>().ok())
        .ok_or_else(|| ApiErrorResponse::unauthorized("Authentication failed"))?;

    req.extensions_mut()
        .insert(DeploymentContext { deployment_id });
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
