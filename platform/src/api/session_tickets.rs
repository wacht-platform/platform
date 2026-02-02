use crate::{application::response::ApiResult, middleware::RequireDeployment};
use axum::{extract::State, Json};
use common::state::AppState;
use commands::{Command, GenerateSessionTicketCommand};
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
    let ticket_type = match request.ticket_type.as_str() {
        "impersonation" => commands::session_ticket::SessionTicketType::Impersonation,
        "agent_access" => commands::session_ticket::SessionTicketType::AgentAccess,
        _ => {
            return Err(crate::application::AppError::BadRequest(
                "Invalid ticket_type. Must be 'impersonation' or 'agent_access'".to_string(),
            )
            .into());
        }
    };

    let mut command = GenerateSessionTicketCommand::new(deployment_id, ticket_type.clone());

    // Add type-specific fields
    match ticket_type {
        commands::session_ticket::SessionTicketType::Impersonation => {
            if let Some(user_id) = request.user_id {
                command = command.with_user_id(user_id);
            } else {
                return Err(crate::application::AppError::BadRequest(
                    "user_id is required for impersonation tickets".to_string(),
                )
                .into());
            }
        }
        commands::session_ticket::SessionTicketType::AgentAccess => {
            if let Some(agent_ids) = request.agent_ids {
                if agent_ids.is_empty() {
                    return Err(crate::application::AppError::BadRequest(
                        "agent_ids cannot be empty for agent_access tickets".to_string(),
                    )
                    .into());
                }
                command = command.with_agent_ids(agent_ids);
            } else {
                return Err(crate::application::AppError::BadRequest(
                    "agent_ids is required for agent_access tickets".to_string(),
                )
                .into());
            }
        }
    }

    if let Some(context_group) = request.context_group {
        command = command.with_context_group(context_group);
    }

    if let Some(expires_in) = request.expires_in {
        command = command.with_expires_in(expires_in);
    }

    let resp = command.execute(&app_state).await?;

    Ok(CreateSessionTicketResponse {
        ticket: resp.ticket,
        expires_at: resp.expires_at,
    }
    .into())
}
