use crate::middleware::RequireDeployment;
use axum::extract::{Json, State};

use crate::{
    application::{HttpState, response::ApiResult},
    core::{
        commands::{Command, CreateExecutionContextCommand},
        dto::json::deployment::CreateExecutionContextRequest,
        models::AgentExecutionContext,
    },
};

pub async fn create_execution_context(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(_request): Json<CreateExecutionContextRequest>,
) -> ApiResult<AgentExecutionContext> {
    CreateExecutionContextCommand::new(deployment_id)
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}
