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

fn bad_request(message: &str) -> AppError {
    AppError::BadRequest(message.to_string())
}

fn command_deployment_id(
    ticket_type: &SessionTicketType,
    deployment_id: i64,
    console_deployment_id: i64,
) -> i64 {
    match ticket_type {
        SessionTicketType::WebhookAppAccess | SessionTicketType::ApiAuthAccess => {
            console_deployment_id
        }
        SessionTicketType::Impersonation | SessionTicketType::AgentAccess => deployment_id,
    }
}

fn require_non_empty_string(
    value: Option<String>,
    required_message: &str,
    empty_message: &str,
) -> Result<String, AppError> {
    let value = value.ok_or_else(|| bad_request(required_message))?;
    if value.is_empty() {
        return Err(bad_request(empty_message));
    }
    Ok(value)
}

fn require_non_empty_agent_ids(value: Option<Vec<String>>) -> Result<Vec<String>, AppError> {
    let value = value.ok_or_else(|| bad_request("agent_ids is required for agent_access tickets"))?;
    if value.is_empty() {
        return Err(bad_request("agent_ids cannot be empty for agent_access tickets"));
    }
    Ok(value)
}

fn apply_ticket_type_fields(
    mut command: GenerateSessionTicketCommand,
    ticket_type: &SessionTicketType,
    request: &mut CreateSessionTicketRequest,
) -> Result<GenerateSessionTicketCommand, AppError> {
    match ticket_type {
        SessionTicketType::Impersonation => {
            let user_id = request
                .user_id
                .take()
                .ok_or_else(|| bad_request("user_id is required for impersonation tickets"))?;
            command = command.with_user_id(user_id);
        }
        SessionTicketType::AgentAccess => {
            let agent_ids = require_non_empty_agent_ids(request.agent_ids.take())?;
            command = command.with_agent_ids(agent_ids);

            if let Some(identifier) = request.agent_session_identifier.take() {
                command = command.with_agent_session_identifier(map_agent_session_identifier(identifier));
            }
        }
        SessionTicketType::WebhookAppAccess => {
            let webhook_app_slug = require_non_empty_string(
                request.webhook_app_slug.take(),
                "webhook_app_slug is required for webhook_app_access tickets",
                "webhook_app_slug cannot be empty for webhook_app_access tickets",
            )?;
            command = command.with_webhook_app_slug(webhook_app_slug);
        }
        SessionTicketType::ApiAuthAccess => {
            let api_auth_app_slug = require_non_empty_string(
                request.api_auth_app_slug.take(),
                "api_auth_app_slug is required for api_auth_access tickets",
                "api_auth_app_slug cannot be empty for api_auth_access tickets",
            )?;
            command = command.with_api_auth_app_slug(api_auth_app_slug);
        }
    }

    Ok(command)
}

pub async fn create_session_ticket(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(mut request): Json<CreateSessionTicketRequest>,
) -> ApiResult<CreateSessionTicketResponse> {
    let ticket_type = parse_ticket_type(&request.ticket_type)?;
    let console_deployment_id = parse_console_deployment_id()?;
    let command_deployment_id =
        command_deployment_id(&ticket_type, deployment_id, console_deployment_id);

    let command = GenerateSessionTicketCommand::new(command_deployment_id, ticket_type.clone());
    let mut command = apply_ticket_type_fields(command, &ticket_type, &mut request)?;

    if let Some(context_group) = request.context_group.take() {
        command = command.with_context_group(context_group);
    }

    if let Some(expires_in) = request.expires_in.take() {
        command = command.with_expires_in(expires_in);
    }

    let resp = command.execute(&app_state).await?;

    Ok(CreateSessionTicketResponse {
        ticket: resp.ticket,
        expires_at: resp.expires_at,
    }
    .into())
}
