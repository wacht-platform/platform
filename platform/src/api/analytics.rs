use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracing::instrument;

use crate::application::analytics::{
    AnalyticsStatsResponse, get_analytics_stats as run_get_analytics_stats,
};
use crate::middleware::RequireDeployment;
use common::state::AppState;

#[derive(Debug, Deserialize)]
pub struct AnalyticsQuery {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

#[instrument(skip(app_state))]
pub async fn get_analytics_stats(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<AnalyticsQuery>,
) -> Result<Json<AnalyticsStatsResponse>, StatusCode> {
    let stats = run_get_analytics_stats(&app_state, deployment_id, query.from, query.to).await?;
    Ok(Json(stats))
}
