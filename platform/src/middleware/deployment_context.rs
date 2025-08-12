use axum::{
    body::Body,
    extract::Request,
    http::StatusCode,
    response::Response,
};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::{Layer, Service};
use tracing::{debug, warn};
use sha2::{Sha256, Digest};
use shared::{
    queries::{Query, api_key::{GetApiKeyByHashQuery, GetDeploymentByApiKeyQuery}},
    commands::{Command, api_key::UpdateApiKeyLastUsedCommand},
    state::AppState,
};
use chrono::Utc;
use super::api_key_context::ApiKeyContext;

/// Deployment context that gets injected into request extensions
#[derive(Clone, Copy, Debug)]
pub struct DeploymentContext {
    pub deployment_id: i64,
}

/// Layer for extracting deployment ID from path (console API)
#[derive(Clone)]
pub struct ConsoleDeploymentLayer;

impl ConsoleDeploymentLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for ConsoleDeploymentLayer {
    type Service = ConsoleDeploymentService<S>;

    fn layer(&self, inner: S) -> ConsoleDeploymentService<S> {
        ConsoleDeploymentService { inner }
    }
}

/// Service that extracts deployment ID from URL path
#[derive(Clone)]
pub struct ConsoleDeploymentService<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for ConsoleDeploymentService<S>
where
    S: Service<Request<Body>, Response = Response> + Send + 'static + Clone,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response, std::convert::Infallible>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), std::convert::Infallible>> {
        match self.inner.poll_ready(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(_)) => Poll::Ready(Ok(())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn call(&mut self, mut req: Request<Body>) -> Pin<Box<dyn Future<Output = Result<Response, std::convert::Infallible>> + Send>> {
        let mut inner = self.inner.clone();
        let path = req.uri().path().to_string();
        
        Box::pin(async move {
            // Extract deployment_id from path
            // Pattern: /deployments/{deployment_id}/...
            if let Some(deployment_id) = extract_deployment_id_from_path(&path) {
                debug!(deployment_id = deployment_id, "Extracted deployment ID from path");
                req.extensions_mut().insert(DeploymentContext { deployment_id });
            } else {
                warn!(path = %path, "No deployment ID found in path");
            }
            
            match inner.call(req).await {
                Ok(response) => Ok(response),
                Err(_) => Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("Internal server error"))
                    .unwrap()),
            }
        })
    }
}

fn extract_deployment_id_from_path(path: &str) -> Option<i64> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    
    // Looking for pattern: /deployments/{deployment_id}/...
    if segments.len() >= 2 && segments[0] == "deployments" {
        segments[1].parse::<i64>().ok()
    } else {
        None
    }
}

/// Middleware function for backend API that extracts deployment from API key
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
            return Err((
                StatusCode::UNAUTHORIZED,
                "API key required".to_string(),
            ));
        }
    };
    
    // Get app state from request extensions
    let state = req.extensions().get::<AppState>().cloned()
        .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, "App state not found".to_string()))?;
    
    // Hash the provided key
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    let key_hash = format!("{:x}", hasher.finalize());
    
    // Look up the key in database
    let key_data = GetApiKeyByHashQuery::new(key_hash)
        .execute(&state)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?
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
        deployment_id: key_data.deployment_id 
    });
    req.extensions_mut().insert(ApiKeyContext {
        key_id: key_data.id,
        app_id: key_data.app_id,
        permissions: key_data.permissions,
    });
    
    Ok(next.run(req).await)
}