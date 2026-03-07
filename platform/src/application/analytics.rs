use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use common::clickhouse::RecentSignup;
use tracing::error;

use crate::application::AppState;

#[derive(serde::Serialize)]
pub struct DailyAuthMetric {
    pub day: String,
    pub signins: i64,
    pub signups: i64,
}

#[derive(serde::Serialize)]
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
    pub daily_metrics: Vec<DailyAuthMetric>,
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

pub async fn get_analytics_stats(
    app_state: &AppState,
    deployment_id: i64,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<AnalyticsStatsResponse, StatusCode> {
    let clickhouse = &app_state.clickhouse_service;
    let (previous_from, previous_to) = previous_window(from, to);

    let stats = clickhouse
        .get_analytics_stats(deployment_id, from, to, previous_from, previous_to)
        .await
        .map_err(|e| {
            error!(error = ?e, "Failed to get analytics stats");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let daily_metrics = stats
        .get_daily_metrics()
        .into_iter()
        .map(|(day, signins, signups)| DailyAuthMetric {
            day,
            signins: signins as i64,
            signups: signups as i64,
        })
        .collect();

    Ok(AnalyticsStatsResponse {
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
        daily_metrics,
        recent_signups: stats.get_recent_signups(),
        recent_signins: stats.get_recent_signins(),
    })
}
