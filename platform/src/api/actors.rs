use crate::{
    application::{actors as actors_app, response::ApiResult},
    middleware::RequireDeployment,
};
use axum::{Json, extract::State};
use common::state::AppState;
use models::Actor;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CreateActorRequest {
    pub subject_type: String,
    pub external_key: String,
    pub display_name: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn create_actor(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateActorRequest>,
) -> ApiResult<Actor> {
    let actor = actors_app::create_actor(
        &app_state,
        deployment_id,
        actors_app::CreateActorRequest {
            subject_type: request.subject_type,
            external_key: request.external_key,
            display_name: request.display_name,
            metadata: request.metadata,
        },
    )
    .await?;
    Ok(actor.into())
}
