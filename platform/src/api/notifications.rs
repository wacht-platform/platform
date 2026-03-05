use crate::{
    application::{notifications as notifications_app, response::ApiResult},
    middleware::RequireDeployment,
};
use axum::{Json, extract::State};
use common::state::AppState;
use models::notification::Notification;

pub use notifications_app::CreateNotificationRequest;

pub async fn create_notification(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateNotificationRequest>,
) -> ApiResult<Vec<Notification>> {
    notifications_app::create_notification(&state, deployment_id, request).await
}
