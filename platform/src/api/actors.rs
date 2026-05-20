use crate::{
    application::{actors as actors_app, response::ApiResult},
    middleware::RequireDeployment,
};
use axum::{
    Json,
    extract::{Query, State},
};
use common::{db_router::ReadConsistency, state::AppState};
use models::Actor;
use queries::agent_thread_model::GetActorByExternalKeyQuery;
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Deserialize)]
pub struct LookupActorParams {
    pub subject_type: String,
    pub external_key: String,
}

// Wrapper so the JSON response always has an explicit `actor` field — `None`
// serialises to `{"actor": null}` rather than `{}` (which is what `flatten` on a
// bare `Option<Actor>` would produce).
#[derive(Debug, Serialize)]
pub struct LookupActorResponse {
    pub actor: Option<Actor>,
}

pub async fn lookup_actor(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<LookupActorParams>,
) -> ApiResult<LookupActorResponse> {
    let actor =
        GetActorByExternalKeyQuery::new(deployment_id, params.subject_type, params.external_key)
            .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
            .await?;
    Ok(LookupActorResponse { actor }.into())
}
