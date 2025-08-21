use axum::{
    extract::{Path, Request},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use serde::Deserialize;
use tracing::debug;

use super::deployment_context::DeploymentContext;

/// Path extractor that captures deployment_id and any additional path params
#[derive(Debug, Deserialize)]
pub struct DeploymentPathParams {
    pub deployment_id: i64,
    #[serde(flatten)]
    pub _rest: std::collections::HashMap<String, serde_json::Value>,
}

/// Axum middleware for console API that extracts deployment_id from path
/// and injects it into request extensions
pub async fn console_deployment_middleware(
    Path(params): Path<DeploymentPathParams>,
    mut req: Request,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    debug!(
        deployment_id = params.deployment_id,
        "Extracted deployment ID from path"
    );

    // Insert deployment context into request extensions
    req.extensions_mut().insert(DeploymentContext {
        deployment_id: params.deployment_id,
    });

    Ok(next.run(req).await)
}
