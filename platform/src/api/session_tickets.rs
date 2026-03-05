use crate::{
    application::{AppError, response::ApiResult},
    middleware::RequireDeployment,
};
use axum::{Json, extract::State};
use commands::session_ticket::{AgentSessionIdentifier, SessionTicketType};
use commands::{Command, GenerateSessionTicketCommand};
use common::state::AppState;
use dto::json::session_ticket::{AgentSessionIdentifierDto, CreateSessionTicketRequest};

#[derive(Debug, serde::Serialize)]
pub struct CreateSessionTicketResponse {
    pub ticket: String,
    pub expires_at: i64,
}

fn parse_ticket_type(ticket_type: &str) -> Result<SessionTicketType, AppError> {
    match ticket_type {
        "impersonation" => Ok(SessionTicketType::Impersonation),
        "agent_access" => Ok(SessionTicketType::AgentAccess),
        "webhook_app_access" => Ok(SessionTicketType::WebhookAppAccess),
        "api_auth_access" => Ok(SessionTicketType::ApiAuthAccess),
        _ => Err(AppError::BadRequest(
            "Invalid ticket_type. Must be 'impersonation', 'agent_access', 'webhook_app_access', or 'api_auth_access'".to_string(),
        )),
    }
}

fn parse_console_deployment_id() -> Result<i64, AppError> {
    std::env::var("CONSOLE_DEPLOYMENT_ID")
        .map_err(|_| AppError::Internal("CONSOLE_DEPLOYMENT_ID is not set".to_string()))?
        .parse::<i64>()
        .map_err(|e| AppError::Internal(format!("Invalid CONSOLE_DEPLOYMENT_ID: {}", e)))
}

fn map_agent_session_identifier(identifier: AgentSessionIdentifierDto) -> AgentSessionIdentifier {
    match identifier {
        AgentSessionIdentifierDto::Static => AgentSessionIdentifier::Static,
        AgentSessionIdentifierDto::Signin => AgentSessionIdentifier::Signin,
    }
}

pub async fn create_session_ticket(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateSessionTicketRequest>,
) -> ApiResult<CreateSessionTicketResponse> {
    let ticket_type = parse_ticket_type(&request.ticket_type)?;
    let console_deployment_id = parse_console_deployment_id()?;

    let mut command = GenerateSessionTicketCommand::new(deployment_id, ticket_type.clone());

    match ticket_type {
        SessionTicketType::Impersonation => {
            if let Some(user_id) = request.user_id {
                command = command.with_user_id(user_id);
            } else {
                return Err(crate::application::AppError::BadRequest(
                    "user_id is required for impersonation tickets".to_string(),
                )
                .into());
            }
        }
        SessionTicketType::AgentAccess => {
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

            if let Some(identifier) = request.agent_session_identifier {
                let mode = map_agent_session_identifier(identifier);
                command = command.with_agent_session_identifier(mode);
            }
        }
        SessionTicketType::WebhookAppAccess => {
            command = GenerateSessionTicketCommand::new(console_deployment_id, ticket_type.clone());
            if let Some(webhook_app_slug) = request.webhook_app_slug {
                if webhook_app_slug.is_empty() {
                    return Err(crate::application::AppError::BadRequest(
                        "webhook_app_slug cannot be empty for webhook_app_access tickets"
                            .to_string(),
                    )
                    .into());
                }
                command = command.with_webhook_app_slug(webhook_app_slug);
            } else {
                return Err(crate::application::AppError::BadRequest(
                    "webhook_app_slug is required for webhook_app_access tickets".to_string(),
                )
                .into());
            }
        }
        SessionTicketType::ApiAuthAccess => {
            command = GenerateSessionTicketCommand::new(console_deployment_id, ticket_type.clone());
            if let Some(api_auth_app_slug) = request.api_auth_app_slug {
                if api_auth_app_slug.is_empty() {
                    return Err(crate::application::AppError::BadRequest(
                        "api_auth_app_slug cannot be empty for api_auth_access tickets".to_string(),
                    )
                    .into());
                }
                command = command.with_api_auth_app_slug(api_auth_app_slug);
            } else {
                return Err(crate::application::AppError::BadRequest(
                    "api_auth_app_slug is required for api_auth_access tickets".to_string(),
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
