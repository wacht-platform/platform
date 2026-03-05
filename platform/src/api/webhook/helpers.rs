use axum::http::StatusCode;
use common::state::AppState;
use models::webhook::WebhookApp;
use queries::{GetWebhookAppByNameQuery, Query as QueryTrait};

use crate::application::response::ApiErrorResponse;

pub(super) async fn get_webhook_app_or_404(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
) -> Result<WebhookApp, ApiErrorResponse> {
    GetWebhookAppByNameQuery::new(deployment_id, app_slug)
        .execute(app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Webhook app not found".to_string()).into())
}

pub(super) async fn ensure_webhook_app_exists(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
) -> Result<(), ApiErrorResponse> {
    get_webhook_app_or_404(app_state, deployment_id, app_slug).await?;
    Ok(())
}
