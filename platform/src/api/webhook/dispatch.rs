use axum::{
    Json,
    extract::{Path, State},
};
use common::state::AppState;
use dto::json::webhook_requests::{TriggerWebhookEventRequest, TriggerWebhookEventResponse};

use crate::application::{response::ApiResult, webhook_dispatch as webhook_dispatch_app};
use crate::middleware::{AppSlugParams, RequireDeployment};

pub async fn trigger_webhook_event(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(AppSlugParams { app_slug, .. }): Path<AppSlugParams>,
    Json(request): Json<TriggerWebhookEventRequest>,
) -> ApiResult<TriggerWebhookEventResponse> {
    let response =
        webhook_dispatch_app::trigger_webhook_event(&app_state, deployment_id, app_slug, request)
            .await?;

    Ok(response.into())
}
