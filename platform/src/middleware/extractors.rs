use axum::{extract::FromRequestParts, http::request::Parts};
use std::sync::LazyLock;

use super::{deployment_context::DeploymentContext, platform_source::PlatformSource};
use crate::application::response::ApiErrorResponse;

/// Extractor that requires deployment context to be present.
///
/// This will be injected by either ConsoleDeploymentLayer (for console API)
/// or backend_deployment_middleware (for backend API).
///
/// # Example
/// ```ignore
/// async fn handler(
///     RequireDeployment(deployment_id): RequireDeployment,
///     // other parameters...
/// ) -> impl IntoResponse {
///     println!("Deployment ID: {}", deployment_id);
///     // handle request...
/// }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct RequireDeployment(pub i64);

impl<S> FromRequestParts<S> for RequireDeployment
where
    S: Send + Sync,
{
    type Rejection = ApiErrorResponse;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<DeploymentContext>()
            .map(|ctx| RequireDeployment(ctx.deployment_id))
            .ok_or_else(|| ApiErrorResponse::bad_request("Deployment context not found"))
    }
}

/// Environment-based extractor for console deployment ID.
///
/// This extractor reads the CONSOLE_DEPLOYMENT_ID environment variable once
/// and caches it using LazyLock for efficient access.
///
/// # Example
/// ```ignore
/// async fn handler(
///     ConsoleDeployment(console_id): ConsoleDeployment,
///     RequireDeployment(customer_id): RequireDeployment,
/// ) -> impl IntoResponse {
///     println!("Console deployment: {}, Customer deployment: {}", console_id, customer_id);
///     // handle request...
/// }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct ConsoleDeployment(pub i64);

static CONSOLE_DEPLOYMENT_ID: LazyLock<Result<i64, String>> = LazyLock::new(|| {
    std::env::var("CONSOLE_DEPLOYMENT_ID")
        .map_err(|_| "CONSOLE_DEPLOYMENT_ID environment variable not set".to_string())
        .and_then(|val| {
            val.parse::<i64>()
                .map_err(|e| format!("Invalid CONSOLE_DEPLOYMENT_ID: {}", e))
        })
});

impl<S> FromRequestParts<S> for ConsoleDeployment
where
    S: Send + Sync,
{
    type Rejection = ApiErrorResponse;

    async fn from_request_parts(_parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        CONSOLE_DEPLOYMENT_ID
            .as_ref()
            .map(|id| ConsoleDeployment(*id))
            .map_err(|e| ApiErrorResponse::internal(e.clone()))
    }
}

/// Extracts whether the request came through the console API or backend API.
#[derive(Debug, Clone, Copy)]
pub struct ExtractPlatformSource(pub PlatformSource);

impl<S> FromRequestParts<S> for ExtractPlatformSource
where
    S: Send + Sync,
{
    type Rejection = ApiErrorResponse;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<PlatformSource>()
            .copied()
            .map(ExtractPlatformSource)
            .ok_or_else(|| ApiErrorResponse::internal("Platform source not found"))
    }
}
