use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{error, instrument};

use crate::middleware::RequireDeployment;
use common::clickhouse::RecentSignup;
use common::state::AppState;

#[derive(Debug, Deserialize)]
pub struct AnalyticsQuery {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct AnalyticsStatsResponse {
    pub unique_signins: i64,
    pub signups: i64,
    pub organizations_created: i64,
    pub workspaces_created: i64,
    pub total_signups: i64,
    pub unique_signins_change: Option<f64>,
    pub signups_change: Option<f64>,
    pub organizations_created_change: Option<f64>,
    pub workspaces_created_change: Option<f64>,
    pub recent_signups: Vec<RecentSignup>,
    pub recent_signins: Vec<RecentSignup>,
}

fn previous_window(from: DateTime<Utc>, to: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let duration = to.signed_duration_since(from);
    (from - duration, to - duration)
}

fn calculate_change(current: i64, previous: i64) -> Option<f64> {
    if previous == 0 {
        if current > 0 { Some(100.0) } else { None }
    } else {
        Some(((current - previous) as f64 / previous as f64) * 100.0)
    }
}

#[instrument(skip(app_state))]
pub async fn get_analytics_stats(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<AnalyticsQuery>,
) -> Result<Json<AnalyticsStatsResponse>, StatusCode> {
    let clickhouse = &app_state.clickhouse_service;
    let (previous_from, previous_to) = previous_window(query.from, query.to);

    let stats = clickhouse
        .get_analytics_stats(
            deployment_id,
            query.from,
            query.to,
            previous_from,
            previous_to,
        )
        .await
        .map_err(|e| {
            error!(error = ?e, "Failed to get analytics stats");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let recent_signups = stats.get_recent_signups();
    let recent_signins = stats.get_recent_signins();

    Ok(Json(AnalyticsStatsResponse {
        unique_signins: stats.unique_signins as i64,
        signups: stats.signups as i64,
        organizations_created: stats.organizations_created as i64,
        workspaces_created: stats.workspaces_created as i64,
        total_signups: stats.total_signups as i64,
        unique_signins_change: calculate_change(
            stats.unique_signins as i64,
            stats.previous_signins as i64,
        ),
        signups_change: calculate_change(stats.signups as i64, stats.previous_signups as i64),
        organizations_created_change: calculate_change(
            stats.organizations_created as i64,
            stats.previous_orgs as i64,
        ),
        workspaces_created_change: calculate_change(
            stats.workspaces_created as i64,
            stats.previous_workspaces as i64,
        ),
        recent_signups,
        recent_signins,
    }))
}
