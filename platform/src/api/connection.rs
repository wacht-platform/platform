use crate::{
    application::{
        response::{ApiResult, ApiSuccess, PaginatedResponse},
    },
    middleware::RequireDeployment,
};
use common::state::AppState;

use axum::{Json, extract::State};
use commands::{Command, UpsertDeploymentSocialConnectionCommand};
use dto::json::DeploymentSocialConnectionUpsert;
use models::DeploymentSocialConnection;
use queries::{Query, deployment::GetDeploymentSocialConnectionsQuery};

pub async fn get_deployment_social_connections(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<PaginatedResponse<DeploymentSocialConnection>> {
    GetDeploymentSocialConnectionsQuery::new(deployment_id)
        .execute(&app_state)
        .await
        .map(Into::<PaginatedResponse<_>>::into)
        .map(ApiSuccess::from)
        .map_err(Into::into)
}

pub async fn upsert_deployment_social_connection(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(payload): Json<DeploymentSocialConnectionUpsert>,
) -> ApiResult<DeploymentSocialConnection> {
    UpsertDeploymentSocialConnectionCommand::new(deployment_id, payload)
        .execute(&app_state)
        .await
        .map(Into::<DeploymentSocialConnection>::into)
        .map(ApiSuccess::from)
        .map_err(Into::into)
}
