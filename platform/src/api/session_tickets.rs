use crate::{
    application::{response::ApiResult, session_tickets as session_tickets_app},
    middleware::RequireDeployment,
};
use axum::{Json, extract::State};
use common::state::AppState;
use dto::json::session_ticket::CreateSessionTicketRequest;

#[derive(Debug, serde::Serialize)]
pub struct CreateSessionTicketResponse {
    pub ticket: String,
    pub expires_at: i64,
}

pub async fn create_session_ticket(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateSessionTicketRequest>,
) -> ApiResult<CreateSessionTicketResponse> {
    let resp =
        session_tickets_app::create_session_ticket(&app_state, deployment_id, request, false)
            .await?;

    Ok(CreateSessionTicketResponse {
        ticket: resp.ticket,
        expires_at: resp.expires_at,
    }
    .into())
}

pub async fn create_backend_session_ticket(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateSessionTicketRequest>,
) -> ApiResult<CreateSessionTicketResponse> {
    let resp =
        session_tickets_app::create_session_ticket(&app_state, deployment_id, request, true)
            .await?;

    Ok(CreateSessionTicketResponse {
        ticket: resp.ticket,
        expires_at: resp.expires_at,
    }
    .into())
}
