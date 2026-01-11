use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::Response,
};
use common::state::AppState;

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

    let identifier = req.uri().path().trim_start_matches('/').replace('/', ":");

    match wacht::gateway::verify_request(api_key, &identifier).await {
        Ok(response) => {
            if !response.allowed {
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    format!(
                        "Rate limit exceeded. Retry after {} seconds",
                        response.retry_after.unwrap_or(60)
                    ),
                ));
            }

            req.extensions_mut().insert(DeploymentContext {
                deployment_id: response.deployment_id,
            });
            req.extensions_mut().insert(ApiKeyContext {
                key_id: response.key_id,
                app_name: response.app_name,
                permissions: response.permissions,
            });

            Ok(next.run(req).await)
        }
        Err(e) => Err((
            StatusCode::UNAUTHORIZED,
            format!("Authentication failed: {}", e),
        )),
    }
}
