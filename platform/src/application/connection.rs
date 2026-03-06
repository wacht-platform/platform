use commands::{Command, UpsertDeploymentSocialConnectionCommand};
use dto::json::DeploymentSocialConnectionUpsert;
use models::DeploymentSocialConnection;
use queries::deployment::GetDeploymentSocialConnectionsQuery;

use crate::application::{AppError, AppState};

pub async fn get_deployment_social_connections(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<Vec<DeploymentSocialConnection>, AppError> {
    GetDeploymentSocialConnectionsQuery::builder()
        .deployment_id(deployment_id)
        .build()?
        .execute_with(app_state.db_router.writer())
        .await
}

pub async fn upsert_deployment_social_connection(
    app_state: &AppState,
    deployment_id: i64,
    payload: DeploymentSocialConnectionUpsert,
) -> Result<DeploymentSocialConnection, AppError> {
    UpsertDeploymentSocialConnectionCommand::builder()
        .deployment_id(deployment_id)
        .connection(payload)
        .build()?
        .execute(app_state)
        .await
}
