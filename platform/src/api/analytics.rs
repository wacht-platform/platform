use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{application::HttpState, core::services::clickhouse::RecentSignup};
use crate::middleware::RequireDeployment;

#[derive(Deserialize)]
pub struct AnalyticsQuery {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct RecentSignupsQuery {
    pub limit: Option<i32>,
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
}

#[derive(Serialize)]
pub struct RecentSignupsResponse {
    pub signups: Vec<RecentSignup>,
}

pub async fn get_analytics_stats(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<AnalyticsQuery>,
) -> Result<Json<AnalyticsStatsResponse>, StatusCode> {
    let clickhouse = &app_state.clickhouse_service;

    let duration = query.to.signed_duration_since(query.from);

    let previous_from = query.from - duration;
    let previous_to = query.to - duration;

    let unique_signins = clickhouse
        .get_unique_signins(deployment_id, query.from, query.to)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let signups = clickhouse
        .get_signups(deployment_id, query.from, query.to)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let organizations_created = clickhouse
        .get_organizations_created(deployment_id, query.from, query.to)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let workspaces_created = clickhouse
        .get_workspaces_created(deployment_id, query.from, query.to)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total_signups = clickhouse
        .get_total_signups(deployment_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let previous_signins = clickhouse
        .get_unique_signins(deployment_id, previous_from, previous_to)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let previous_signups = clickhouse
        .get_signups(deployment_id, previous_from, previous_to)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let previous_orgs = clickhouse
        .get_organizations_created(deployment_id, previous_from, previous_to)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let previous_workspaces = clickhouse
        .get_workspaces_created(deployment_id, previous_from, previous_to)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let calculate_change = |current: i64, previous: i64| -> Option<f64> {
        if previous == 0 {
            if current > 0 { Some(100.0) } else { None }
        } else {
            Some(((current - previous) as f64 / previous as f64) * 100.0)
        }
    };

    Ok(Json(AnalyticsStatsResponse {
        unique_signins,
        signups,
        organizations_created,
        workspaces_created,
        total_signups,
        unique_signins_change: calculate_change(unique_signins, previous_signins),
        signups_change: calculate_change(signups, previous_signups),
        organizations_created_change: calculate_change(organizations_created, previous_orgs),
        workspaces_created_change: calculate_change(workspaces_created, previous_workspaces),
    }))
}

pub async fn get_recent_signups(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<RecentSignupsQuery>,
) -> Result<Json<RecentSignupsResponse>, StatusCode> {
    let limit = query.limit.unwrap_or(10);

    let signups = app_state
        .clickhouse_service
        .get_recent_signups(deployment_id, limit)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(RecentSignupsResponse { signups }))
}
