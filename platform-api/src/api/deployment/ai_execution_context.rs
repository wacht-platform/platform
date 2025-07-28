use axum::extract::{Json, Path, State};

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
    Path(deployment_id): Path<i64>,
    Json(_request): Json<CreateExecutionContextRequest>,
) -> ApiResult<AgentExecutionContext> {
    CreateExecutionContextCommand::new(deployment_id)
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}
