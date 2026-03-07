use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
};
use common::state::AppState;

use crate::application::billing_webhook as billing_webhook_app;

pub async fn handle_dodo_webhook(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Result<StatusCode, StatusCode> {
    billing_webhook_app::handle_dodo_webhook(&app_state, &headers, &body).await
}
