use axum::{
    Json,
    extract::{Path, Query, State},
};
use common::state::AppState;
use dto::json::webhook_requests::{
    ReplayTaskCancelResponse, ReplayTaskListQuery, ReplayTaskListResponse,
    ReplayTaskStatusResponse, ReplayWebhookDeliveryRequest, ReplayWebhookDeliveryResponse,
};

use crate::application::{response::ApiResult, webhook_replay as webhook_replay_app};
use crate::middleware::RequireDeployment;

pub async fn replay_webhook_delivery(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Json(request): Json<ReplayWebhookDeliveryRequest>,
) -> ApiResult<ReplayWebhookDeliveryResponse> {
    let response = webhook_replay_app::replay_webhook_delivery(
        &app_state,
        deployment_id,
        app_slug,
        request,
    )
    .await?;

    Ok(response.into())
}

pub async fn get_webhook_replay_task_status(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, task_id)): Path<(String, String)>,
) -> ApiResult<ReplayTaskStatusResponse> {
    let response = webhook_replay_app::get_webhook_replay_task_status(
        &app_state,
        deployment_id,
        app_slug,
        task_id,
    )
    .await?;

    Ok(response.into())
}

pub async fn cancel_webhook_replay_task(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((app_slug, task_id)): Path<(String, String)>,
) -> ApiResult<ReplayTaskCancelResponse> {
    let response = webhook_replay_app::cancel_webhook_replay_task(
        &app_state,
        deployment_id,
        app_slug,
        task_id,
    )
    .await?;

    Ok(response.into())
}

pub async fn list_webhook_replay_tasks(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<ReplayTaskListQuery>,
) -> ApiResult<ReplayTaskListResponse> {
    let response = webhook_replay_app::list_webhook_replay_tasks(
        &app_state,
        deployment_id,
        app_slug,
        params,
    )
    .await?;

    Ok(response.into())
}
