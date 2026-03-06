use axum::extract::{Path, Query, State};

use crate::application::{api_key_audit as api_key_audit_use_cases, response::ApiResult};
use crate::middleware::RequireDeployment;
use common::state::AppState;
use dto::json::api_key::*;

pub async fn get_api_audit_logs(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<ListApiAuditLogsQuery>,
) -> ApiResult<ApiAuditLogsResponse> {
    let result = api_key_audit_use_cases::get_api_audit_logs(&app_state, deployment_id, app_slug, params)
        .await?;
    Ok(result.into())
}

pub async fn get_api_audit_analytics(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<GetApiAuditAnalyticsQuery>,
) -> ApiResult<ApiAuditAnalyticsResponse> {
    let result =
        api_key_audit_use_cases::get_api_audit_analytics(&app_state, deployment_id, app_slug, params)
            .await?;
    Ok(result.into())
}

pub async fn get_api_audit_timeseries(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<GetApiAuditTimeseriesQuery>,
) -> ApiResult<ApiAuditTimeseriesResponse> {
    let result = api_key_audit_use_cases::get_api_audit_timeseries(
        &app_state,
        deployment_id,
        app_slug,
        params,
    )
    .await?;

    Ok(result.into())
}
