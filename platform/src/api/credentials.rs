use axum::extract::State;
use common::state::AppState;
use dto::json::credentials::DeploymentCredentialsResponse;

use crate::{
    application::{credentials, response::ApiResult},
    middleware::RequireDeployment,
};

pub async fn create_deployment_credentials(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<DeploymentCredentialsResponse> {
    let credentials = credentials::create_deployment_credentials(&app_state, deployment_id).await?;
    Ok(credentials.into())
}
