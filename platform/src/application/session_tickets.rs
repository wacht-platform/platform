use commands::session_ticket::{AgentSessionIdentifier, SessionTicketType};
use commands::{CreateActorCommand, GenerateSessionTicketCommand};
use dto::json::session_ticket::{AgentSessionIdentifierDto, CreateSessionTicketRequest};
use queries::GetActorByIdQuery;

use crate::application::{AppError, AppState};
use common::deps;

const DEBUG_ACTOR_SUBJECT_TYPE: &str = "debug_actor";

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

fn is_static_agent_access_request(
    ticket_type: &SessionTicketType,
    request: &CreateSessionTicketRequest,
) -> bool {
    matches!(ticket_type, SessionTicketType::AgentAccess)
        && matches!(
            request.agent_session_identifier,
            None | Some(AgentSessionIdentifierDto::Static)
        )
}

fn parse_actor_id(actor_id: &str) -> Result<i64, AppError> {
    actor_id
        .parse::<i64>()
        .map_err(|_| bad_request("actor_id must be a valid integer"))
}

async fn ensure_debug_actor_exists(
    app_state: &AppState,
    deployment_id: i64,
    actor_id: i64,
) -> Result<(), AppError> {
    if GetActorByIdQuery::new(actor_id, deployment_id)
        .execute_with_db(app_state.db_router.writer())
        .await?
        .is_some()
    {
        return Ok(());
    }

    let create_actor = CreateActorCommand::new(
        actor_id,
        deployment_id,
        DEBUG_ACTOR_SUBJECT_TYPE.to_string(),
        actor_id.to_string(),
    )
    .with_display_name(format!("Debug Actor {}", actor_id))
    .with_metadata(serde_json::json!({
        "auto_created": true,
        "source": "console_debug_session",
    }));

    match create_actor
        .execute_with_db(app_state.db_router.writer())
        .await
    {
        Ok(_) => Ok(()),
        Err(AppError::Database(db_err))
            if db_err
                .as_database_error()
                .is_some_and(|db_err| db_err.code().as_deref() == Some("23505")) =>
        {
            if GetActorByIdQuery::new(actor_id, deployment_id)
                .execute_with_db(app_state.db_router.writer())
                .await?
                .is_some()
            {
                Ok(())
            } else {
                Err(AppError::Conflict(format!(
                    "actor_id {} is already in use",
                    actor_id
                )))
            }
        }
        Err(error) => Err(error),
    }
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
    let value =
        value.ok_or_else(|| bad_request("agent_ids is required for agent_access tickets"))?;
    if value.is_empty() {
        return Err(bad_request(
            "agent_ids cannot be empty for agent_access tickets",
        ));
    }
    Ok(value)
}

fn require_non_empty_actor_id(value: Option<String>) -> Result<String, AppError> {
    require_non_empty_string(
        value,
        "actor_id is required for static agent_access tickets",
        "actor_id cannot be empty for static agent_access tickets",
    )
}

fn apply_ticket_type_fields(
    mut command: commands::session_ticket::GenerateSessionTicketCommandBuilder,
    ticket_type: &SessionTicketType,
    request: &mut CreateSessionTicketRequest,
) -> Result<commands::session_ticket::GenerateSessionTicketCommandBuilder, AppError> {
    match ticket_type {
        SessionTicketType::Impersonation => {
            let user_id = request
                .user_id
                .take()
                .ok_or_else(|| bad_request("user_id is required for impersonation tickets"))?;
            command = command.user_id(user_id);
        }
        SessionTicketType::AgentAccess => {
            let agent_ids = require_non_empty_agent_ids(request.agent_ids.take())?;
            command = command.agent_ids(agent_ids);

            if let Some(identifier) = request.agent_session_identifier.take() {
                command =
                    command.agent_session_identifier(map_agent_session_identifier(identifier));
            }
        }
        SessionTicketType::WebhookAppAccess => {
            let webhook_app_slug = require_non_empty_string(
                request.webhook_app_slug.take(),
                "webhook_app_slug is required for webhook_app_access tickets",
                "webhook_app_slug cannot be empty for webhook_app_access tickets",
            )?;
            command = command.webhook_app_slug(webhook_app_slug);
        }
        SessionTicketType::ApiAuthAccess => {
            let api_auth_app_slug = require_non_empty_string(
                request.api_auth_app_slug.take(),
                "api_auth_app_slug is required for api_auth_access tickets",
                "api_auth_app_slug cannot be empty for api_auth_access tickets",
            )?;
            command = command.api_auth_app_slug(api_auth_app_slug);
        }
    }

    Ok(command)
}

pub async fn create_session_ticket(
    app_state: &AppState,
    deployment_id: i64,
    mut request: CreateSessionTicketRequest,
) -> Result<commands::session_ticket::GenerateSessionTicketResponse, AppError> {
    let session_deps = deps::from_app(app_state).redis().id();
    let ticket_type = parse_ticket_type(&request.ticket_type)?;
    let console_deployment_id = parse_console_deployment_id()?;
    let command_deployment_id =
        command_deployment_id(&ticket_type, deployment_id, console_deployment_id);

    if is_static_agent_access_request(&ticket_type, &request) {
        let actor_id = require_non_empty_actor_id(request.actor_id.clone())?;
        ensure_debug_actor_exists(app_state, deployment_id, parse_actor_id(&actor_id)?).await?;
        request.actor_id = Some(actor_id);
    }

    let command = GenerateSessionTicketCommand::builder()
        .deployment_id(command_deployment_id)
        .ticket_type(ticket_type.clone());
    let mut command = apply_ticket_type_fields(command, &ticket_type, &mut request)?;

    if let Some(actor_id) = request.actor_id.take() {
        command = command.actor_id(actor_id);
    }

    if let Some(expires_in) = request.expires_in.take() {
        command = command.expires_in(expires_in);
    }

    command.build()?.execute_with_deps(&session_deps).await
}
