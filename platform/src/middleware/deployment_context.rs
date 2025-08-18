#[cfg(feature = "backend-api")]
use super::api_key_context::ApiKeyContext;
#[cfg(feature = "backend-api")]
use axum::{body::Body, extract::Request, http::StatusCode, response::Response};
#[cfg(feature = "backend-api")]
use chrono::Utc;
#[cfg(feature = "backend-api")]
use sha2::{Digest, Sha256};
#[cfg(feature = "backend-api")]
use commands::{Command, api_key::UpdateApiKeyLastUsedCommand};
#[cfg(feature = "backend-api")]
use queries::{Query, api_key::GetApiKeyByHashQuery};
#[cfg(feature = "backend-api")]
use common::state::AppState;

/// Deployment context that gets injected into request extensions
#[derive(Clone, Copy, Debug)]
pub struct DeploymentContext {
    pub deployment_id: i64,
}

#[cfg(feature = "backend-api")]
pub async fn backend_deployment_middleware(
    mut req: Request<Body>,
    next: axum::middleware::Next,
) -> Result<Response, (StatusCode, String)> {
    // Try to extract API key from headers
    let api_key = req
        .headers()
        .get("x-api-key")
        .or_else(|| req.headers().get("authorization"))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or(Some(v)));

    let api_key = match api_key {
        Some(key) => key,
        None => {
            return Err((StatusCode::UNAUTHORIZED, "API key required".to_string()));
        }
    };

    // Get app state from request extensions
    let state = req.extensions().get::<AppState>().cloned().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "App state not found".to_string(),
        )
    })?;

    // Hash the provided key
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    let key_hash = format!("{:x}", hasher.finalize());

    // Look up the key in database
    let key_data = GetApiKeyByHashQuery::new(key_hash)
        .execute(&state)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, "Invalid API key".to_string()))?;

    // Check if key is active
    if !key_data.is_active {
        return Err((StatusCode::UNAUTHORIZED, "API key is revoked".to_string()));
    }

    // Check expiration
    if let Some(expires_at) = key_data.expires_at {
        if expires_at < Utc::now() {
            return Err((StatusCode::UNAUTHORIZED, "API key has expired".to_string()));
        }
    }

    // Update last_used_at asynchronously
    let key_id = key_data.id;
    let state_clone = state.clone();
    tokio::spawn(async move {
        let _ = UpdateApiKeyLastUsedCommand { key_id }
            .execute(&state_clone)
            .await;
    });

    // Inject contexts
    req.extensions_mut().insert(DeploymentContext {
        deployment_id: key_data.deployment_id,
    });
    req.extensions_mut().insert(ApiKeyContext {
        key_id: key_data.id,
        app_id: key_data.app_id,
        permissions: key_data.permissions,
    });

    Ok(next.run(req).await)
}
