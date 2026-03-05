use commands::{Command, UpsertDeploymentSocialConnectionCommand};
use dto::json::DeploymentSocialConnectionUpsert;
use models::DeploymentSocialConnection;
use queries::{Query as QueryTrait, deployment::GetDeploymentSocialConnectionsQuery};

use crate::application::{AppError, AppState};

pub async fn get_deployment_social_connections(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<Vec<DeploymentSocialConnection>, AppError> {
    GetDeploymentSocialConnectionsQuery::new(deployment_id)
        .execute(app_state)
        .await
}

pub async fn upsert_deployment_social_connection(
    app_state: &AppState,
    deployment_id: i64,
    payload: DeploymentSocialConnectionUpsert,
) -> Result<DeploymentSocialConnection, AppError> {
    UpsertDeploymentSocialConnectionCommand::new(deployment_id, payload)
        .execute(app_state)
        .await
}
