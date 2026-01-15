use crate::{application::response::ApiResult, middleware::RequireDeployment};
use axum::{extract::State, Json};
use common::state::AppState;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct GenerateAgentTicketRequest {
    pub agent_ids: Vec<String>,
    pub context_group: String,
    pub expires_in: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct GenerateAgentTicketResponse {
    pub ticket: String,
    pub expires_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct TicketPayload {
    deployment_id: String,
    identifier: String,
    context_group: String,
    agent_ids: Vec<String>,
    expires_at: i64,
}

pub async fn generate_agent_ticket(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<GenerateAgentTicketRequest>,
) -> ApiResult<GenerateAgentTicketResponse> {
    use crate::application::AppError;

    if request.agent_ids.is_empty() {
        return Err(AppError::BadRequest("agent_ids is required".to_string()).into());
    }

    if request.context_group.is_empty() {
        return Err(AppError::BadRequest("context_group is required".to_string()).into());
    }

    let ttl_seconds = request.expires_in.unwrap_or(43200); // Default 12 hours
    let ticket_id = app_state
        .sf
        .next_id()
        .map_err(|e| AppError::Internal(format!("Failed to generate ticket ID: {}", e)))?;
    let ticket = format!("{}", ticket_id);
    let expires_at = chrono::Utc::now().timestamp() + ttl_seconds as i64;

    let payload = TicketPayload {
        deployment_id: deployment_id.to_string(),
        identifier: "static".to_string(),
        context_group: request.context_group,
        agent_ids: request.agent_ids,
        expires_at,
    };

    let payload_json = serde_json::to_string(&payload)
        .map_err(|e| AppError::Serialization(format!("Failed to serialize ticket: {}", e)))?;

    let redis_key = format!("agent:ticket:{}", ticket);

    let mut conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to connect to Redis: {}", e)))?;

    conn.set_ex::<_, _, ()>(&redis_key, &payload_json, ttl_seconds)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to store ticket in Redis: {}", e)))?;

    Ok(GenerateAgentTicketResponse { ticket, expires_at }.into())
}
