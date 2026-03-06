use crate::middleware::RequireDeployment;
use axum::extract::{Json, Path, Query, State};
use serde::Deserialize;

use crate::api::pagination::paginate_results;
use crate::application::agent_integrations::{
    create_agent_integration as run_create_agent_integration,
    delete_agent_integration as run_delete_agent_integration, ensure_integrations_beta_enabled,
    get_agent_integration_by_id as run_get_agent_integration_by_id,
    list_agent_integrations as run_list_agent_integrations, normalize_integration_config,
    parse_integration_type, update_agent_integration as run_update_agent_integration,
};
use crate::application::response::{ApiResult, PaginatedResponse};
use common::state::AppState;

use models::AgentIntegration;

fn resolve_pagination(query: &GetIntegrationsQuery) -> (i64, Option<i64>) {
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset;
    (limit, offset)
}

#[derive(Deserialize)]
pub struct AgentIntegrationParams {
    pub agent_id: i64,
    pub integration_id: i64,
}

#[derive(Deserialize)]
pub struct AgentParams {
    pub agent_id: i64,
}

#[derive(Deserialize)]
pub struct GetIntegrationsQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Deserialize)]
pub struct CreateIntegrationRequest {
    pub integration_type: String,
    pub name: String,
    pub config: serde_json::Value,
}

#[derive(Deserialize)]
pub struct UpdateIntegrationRequest {
    pub name: Option<String>,
    pub config: Option<serde_json::Value>,
}

/// GET /agents/:agent_id/integrations
pub async fn get_agent_integrations(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
    Query(query): Query<GetIntegrationsQuery>,
) -> ApiResult<PaginatedResponse<AgentIntegration>> {
    let (limit, offset) = resolve_pagination(&query);

    let integrations =
        run_list_agent_integrations(&app_state, deployment_id, params.agent_id, limit, offset)
            .await?;

    Ok(paginate_results(integrations, limit as i32, offset).into())
}

/// POST /agents/:agent_id/integrations
pub async fn create_agent_integration(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
    Json(request): Json<CreateIntegrationRequest>,
) -> ApiResult<AgentIntegration> {
    ensure_integrations_beta_enabled()?;

    let integration_type = parse_integration_type(&request.integration_type)?;
    let normalized_config = normalize_integration_config(integration_type, request.config)?;

    let integration = run_create_agent_integration(
        &app_state,
        deployment_id,
        params.agent_id,
        integration_type,
        request.name,
        normalized_config,
    )
    .await?;
    Ok(integration.into())
}

/// GET /agents/:agent_id/integrations/:integration_id
pub async fn get_agent_integration_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentIntegrationParams>,
) -> ApiResult<AgentIntegration> {
    let integration = run_get_agent_integration_by_id(
        &app_state,
        deployment_id,
        params.agent_id,
        params.integration_id,
    )
    .await?;
    Ok(integration.into())
}

/// PATCH /agents/:agent_id/integrations/:integration_id
pub async fn update_agent_integration(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentIntegrationParams>,
    Json(request): Json<UpdateIntegrationRequest>,
) -> ApiResult<AgentIntegration> {
    let existing_integration_type = if request.config.is_some() {
        Some(
            run_get_agent_integration_by_id(
                &app_state,
                deployment_id,
                params.agent_id,
                params.integration_id,
            )
            .await?
            .integration_type,
        )
    } else {
        None
    };

    let integration = run_update_agent_integration(
        &app_state,
        deployment_id,
        params.integration_id,
        request.name,
        request.config,
        existing_integration_type,
    )
    .await?;
    Ok(integration.into())
}

/// DELETE /agents/:agent_id/integrations/:integration_id
pub async fn delete_agent_integration(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentIntegrationParams>,
) -> ApiResult<()> {
    run_delete_agent_integration(&app_state, deployment_id, params.integration_id).await?;
    Ok(().into())
}
