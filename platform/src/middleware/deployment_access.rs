use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use serde::Deserialize;
use tracing::{debug, warn};
use wacht::middleware::auth::AuthContext;

use super::deployment_context::DeploymentContext;
use common::state::AppState;
use queries::{Query, deployment::GetDeploymentWithProjectQuery};

/// Path extractor that captures deployment_id and any additional path params
#[derive(Debug, Deserialize)]
pub struct DeploymentPathParams {
    pub deployment_id: i64,
    #[serde(flatten)]
    pub _rest: std::collections::HashMap<String, serde_json::Value>,
}

/// Enhanced deployment middleware that:
/// 1. Extracts deployment_id from path
/// 2. Verifies user has access to the deployment's project
/// 3. Injects deployment context into request extensions
pub async fn deployment_access_middleware(
    State(app_state): State<AppState>,
    Path(params): Path<DeploymentPathParams>,
    mut req: Request,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    debug!(
        deployment_id = params.deployment_id,
        "Checking deployment access"
    );

    // Get auth context from request extensions (set by AuthLayer)
    let auth_context = req
        .extensions()
        .get::<AuthContext>()
        .ok_or_else(|| {
            warn!("No auth context found in request");
            (
                StatusCode::UNAUTHORIZED,
                "Authentication required".to_string(),
            )
        })?
        .clone();

    let deployment_with_project = GetDeploymentWithProjectQuery::new(params.deployment_id)
        .execute(&app_state)
        .await
        .map_err(|e| {
            warn!("Failed to get deployment: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to verify access".to_string(),
            )
        })?;

    let deployment_with_project = deployment_with_project.ok_or_else(|| {
        warn!(deployment_id = params.deployment_id, "Deployment not found");
        (StatusCode::NOT_FOUND, "Deployment not found".to_string())
    })?;

    let has_access = match &deployment_with_project.project_owner_id {
        Some(owner_id) => {
            owner_id == &auth_context.user_id
                || auth_context
                    .organization_id
                    .as_ref()
                    .map_or(false, |org_id| owner_id == org_id)
        }
        None => {
            warn!(
                deployment_id = params.deployment_id,
                project_id = deployment_with_project.project_id,
                "Project has no owner, denying access"
            );
            false
        }
    };

    if !has_access {
        warn!(
            user_id = auth_context.user_id,
            deployment_id = params.deployment_id,
            project_owner = ?deployment_with_project.project_owner_id,
            "User attempted to access deployment without permission"
        );
        return Err((
            StatusCode::FORBIDDEN,
            "You don't have permission to access this deployment".to_string(),
        ));
    }

    debug!(
        user_id = auth_context.user_id,
        deployment_id = params.deployment_id,
        "Access granted to deployment"
    );

    req.extensions_mut().insert(DeploymentContext {
        deployment_id: params.deployment_id,
    });

    Ok(next.run(req).await)
}
