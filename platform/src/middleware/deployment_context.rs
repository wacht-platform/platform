use super::api_key_context::ApiKeyContext;
use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::Response,
};
use chrono::Utc;
use commands::{Command, api_key::UpdateApiKeyLastUsedCommand};
use common::state::AppState;
use queries::{Query, api_key::GetApiKeyIdentifiersByHashQuery};
use sha2::{Digest, Sha256};

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

    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    let key_hash = format!("{:x}", hasher.finalize());

    let key_data = GetApiKeyIdentifiersByHashQuery::new(key_hash)
        .execute(&state)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, "Invalid API key".to_string()))?;

    if !key_data.is_active {
        return Err((StatusCode::UNAUTHORIZED, "API key is revoked".to_string()));
    }

    if let Some(expires_at) = key_data.expires_at {
        if expires_at < Utc::now() {
            return Err((StatusCode::UNAUTHORIZED, "API key has expired".to_string()));
        }
    }

    let key_id = key_data.id;
    let state_clone = state.clone();
    tokio::spawn(async move {
        let _ = UpdateApiKeyLastUsedCommand { key_id }
            .execute(&state_clone)
            .await;
    });

    req.extensions_mut().insert(DeploymentContext {
        deployment_id: key_data.app_name.parse().unwrap(),
    });
    req.extensions_mut().insert(ApiKeyContext {
        key_id: key_data.id,
        app_name: key_data.app_name,
        permissions: key_data.permissions,
    });

    Ok(next.run(req).await)
}
