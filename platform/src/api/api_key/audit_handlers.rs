use axum::extract::{Path, Query, State};
use axum::http::StatusCode;

use crate::application::response::ApiResult;
use crate::middleware::RequireDeployment;
use common::state::AppState;
use dto::json::api_key::*;
use queries::{
    Query as QueryTrait,
    api_key::GetApiAuthAppBySlugQuery,
    api_key_audit::{
        GetApiAuditAnalyticsQuery as GetApiAuditAnalyticsDataQuery,
        GetApiAuditLogsQuery as GetApiAuditLogsDataQuery,
        GetApiAuditTimeseriesQuery as GetApiAuditTimeseriesDataQuery,
    },
};

pub async fn get_api_audit_logs(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<ListApiAuditLogsQuery>,
) -> ApiResult<ApiAuditLogsResponse> {
    GetApiAuthAppBySlugQuery::new(deployment_id, app_slug.clone())
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    let mut cursor_ts = params.cursor_ts;
    let mut cursor_id = params.cursor_id.clone();
    if let Some(cursor) = params.cursor {
        use base64::Engine;
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(cursor)
            .map_err(|_| (StatusCode::BAD_REQUEST, "invalid cursor"))?;
        let decoded =
            String::from_utf8(decoded).map_err(|_| (StatusCode::BAD_REQUEST, "invalid cursor"))?;
        let parts: Vec<&str> = decoded.splitn(2, '|').collect();
        if parts.len() != 2 {
            return Err((StatusCode::BAD_REQUEST, "invalid cursor").into());
        }
        let cursor_ms: i64 = parts[0]
            .parse()
            .map_err(|_| (StatusCode::BAD_REQUEST, "invalid cursor"))?;
        cursor_ts = Some(
            chrono::DateTime::from_timestamp_millis(cursor_ms)
                .ok_or((StatusCode::BAD_REQUEST, "invalid cursor"))?,
        );
        cursor_id = Some(parts[1].to_string());
    }

    let result = GetApiAuditLogsDataQuery {
        deployment_id,
        app_slug,
        limit: params.limit.unwrap_or(100),
        offset: params.offset.unwrap_or(0),
        cursor_ts,
        cursor_id,
        outcome: params.outcome,
        key_id: params.key_id.map(|v| v.get()),
        start_date: params.start_date,
        end_date: params.end_date,
    }
    .execute(&app_state)
    .await?;

    Ok(result.into())
}

pub async fn get_api_audit_analytics(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<GetApiAuditAnalyticsQuery>,
) -> ApiResult<ApiAuditAnalyticsResponse> {
    GetApiAuthAppBySlugQuery::new(deployment_id, app_slug.clone())
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    let result = GetApiAuditAnalyticsDataQuery {
        deployment_id,
        app_slug,
        start_date: params.start_date,
        end_date: params.end_date,
        key_id: params.key_id.map(|v| v.get()),
        include_top_keys: params.include_top_keys.unwrap_or(false),
        include_top_paths: params.include_top_paths.unwrap_or(false),
        include_blocked_reasons: params.include_blocked_reasons.unwrap_or(false),
        include_rate_limits: params.include_rate_limits.unwrap_or(false),
        top_limit: params.top_limit.unwrap_or(10),
    }
    .execute(&app_state)
    .await?;

    Ok(result.into())
}

pub async fn get_api_audit_timeseries(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<GetApiAuditTimeseriesQuery>,
) -> ApiResult<ApiAuditTimeseriesResponse> {
    GetApiAuthAppBySlugQuery::new(deployment_id, app_slug.clone())
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;

    let interval = params.interval.unwrap_or_else(|| "hour".to_string());
    let normalized_interval = match interval.as_str() {
        "minute" | "hour" | "day" | "week" | "month" => interval,
        _ => "hour".to_string(),
    };

    let result = GetApiAuditTimeseriesDataQuery {
        deployment_id,
        app_slug,
        start_date: params.start_date,
        end_date: params.end_date,
        interval: normalized_interval,
        key_id: params.key_id.map(|v| v.get()),
    }
    .execute(&app_state)
    .await?;

    Ok(result.into())
}
