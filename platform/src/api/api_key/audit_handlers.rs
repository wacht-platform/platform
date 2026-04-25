use axum::extract::{Path, Query, State};

use crate::application::{api_key_audit as api_key_audit_app, response::ApiResult};
use crate::middleware::{AppSlugParams, RequireDeployment};
use common::state::AppState;
use dto::json::api_key::*;

pub async fn get_api_audit_logs(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(AppSlugParams { app_slug, .. }): Path<AppSlugParams>,
    Query(params): Query<ListApiAuditLogsQuery>,
) -> ApiResult<ApiAuditLogsResponse> {
    let result =
        api_key_audit_app::get_api_audit_logs(&app_state, deployment_id, app_slug, params).await?;
    Ok(result.into())
}

pub async fn get_api_audit_analytics(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(AppSlugParams { app_slug, .. }): Path<AppSlugParams>,
    Query(params): Query<GetApiAuditAnalyticsQuery>,
) -> ApiResult<ApiAuditAnalyticsResponse> {
    let result =
        api_key_audit_app::get_api_audit_analytics(&app_state, deployment_id, app_slug, params)
            .await?;
    Ok(result.into())
}

pub async fn get_api_audit_timeseries(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(AppSlugParams { app_slug, .. }): Path<AppSlugParams>,
    Query(params): Query<GetApiAuditTimeseriesQuery>,
) -> ApiResult<ApiAuditTimeseriesResponse> {
    let result =
        api_key_audit_app::get_api_audit_timeseries(&app_state, deployment_id, app_slug, params)
            .await?;

    Ok(result.into())
}
