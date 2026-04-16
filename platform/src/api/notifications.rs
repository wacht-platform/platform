use crate::{
    application::{notifications as notifications_app, response::ApiResult},
    middleware::RequireDeployment,
};
use axum::{Json, extract::State};
use common::state::AppState;
use models::notification::Notification;
use serde::Serialize;

pub use notifications_app::CreateNotificationRequest;

#[derive(Serialize)]
pub struct CreateNotificationsResponse {
    pub data: Vec<Notification>,
}

pub async fn create_notification(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateNotificationRequest>,
) -> ApiResult<CreateNotificationsResponse> {
    let notifications = notifications_app::create_notification(&state, deployment_id, request)
        .await?
        .data;
    Ok(CreateNotificationsResponse {
        data: notifications,
    }
    .into())
}
