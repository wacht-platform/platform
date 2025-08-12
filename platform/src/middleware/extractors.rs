use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};

use super::deployment_context::DeploymentContext;

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
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<DeploymentContext>()
            .map(|ctx| RequireDeployment(ctx.deployment_id))
            .ok_or((StatusCode::BAD_REQUEST, "Deployment context not found"))
    }
}