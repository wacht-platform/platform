use commands::UpsertDeploymentSocialConnectionCommand;
use common::db_router::ReadConsistency;
use dto::json::DeploymentSocialConnectionUpsert;
use models::DeploymentSocialConnection;
use queries::deployment::GetDeploymentSocialConnectionsQuery;

use crate::application::{AppError, AppState};
use crate::application::deps;

pub async fn get_deployment_social_connections(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<Vec<DeploymentSocialConnection>, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetDeploymentSocialConnectionsQuery::builder()
        .deployment_id(deployment_id)
        .build()?
        .execute_with_db(reader)
        .await
}

pub async fn upsert_deployment_social_connection(
    app_state: &AppState,
    deployment_id: i64,
    payload: DeploymentSocialConnectionUpsert,
) -> Result<DeploymentSocialConnection, AppError> {
    UpsertDeploymentSocialConnectionCommand::builder()
        .social_connection_id(app_state.sf.next_id()? as i64)
        .deployment_id(deployment_id)
        .connection(payload)
        .build()?
        .execute_with_deps(&deps::from_app(app_state).db().redis())
        .await
}
