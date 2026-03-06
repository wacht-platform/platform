use crate::{
    application::connection::{
        get_deployment_social_connections as run_get_deployment_social_connections,
        upsert_deployment_social_connection as run_upsert_deployment_social_connection,
    },
    application::response::{ApiResult, PaginatedResponse},
    middleware::RequireDeployment,
};
use common::state::AppState;

use axum::{Json, extract::State};
use dto::json::DeploymentSocialConnectionUpsert;
use models::DeploymentSocialConnection;

pub async fn get_deployment_social_connections(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<PaginatedResponse<DeploymentSocialConnection>> {
    let connections = run_get_deployment_social_connections(&app_state, deployment_id).await?;
    Ok(PaginatedResponse::from(connections).into())
}

pub async fn upsert_deployment_social_connection(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(payload): Json<DeploymentSocialConnectionUpsert>,
) -> ApiResult<DeploymentSocialConnection> {
    let connection =
        run_upsert_deployment_social_connection(&app_state, deployment_id, payload).await?;
    Ok(connection.into())
}
