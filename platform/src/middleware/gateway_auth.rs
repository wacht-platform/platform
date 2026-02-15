use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::Response,
};
use common::state::AppState;
use wacht::gateway::GatewayDenyReason;

use super::api_key_context::ApiKeyContext;
use super::deployment_context::DeploymentContext;

pub async fn gateway_auth_middleware(
    State(_state): State<AppState>,
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

    let method = req.method().as_str().to_string();
    let resource = req.uri().path().to_string();

    match wacht::gateway::verify_request(api_key, &method, &resource).await {
        Ok(response) => {
            if !response.allowed {
                let (status, message) =
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
                    };
                return Err((status, message));
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
        Err(e) => Err((
            StatusCode::UNAUTHORIZED,
            format!("Authentication failed: {}", e),
        )),
    }
}
