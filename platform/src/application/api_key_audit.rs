use base64::Engine;
use common::state::AppState;
use dto::json::api_key::{
    ApiAuditAnalyticsResponse, ApiAuditLogsResponse, ApiAuditTimeseriesResponse,
    GetApiAuditAnalyticsQuery, GetApiAuditTimeseriesQuery, ListApiAuditLogsQuery,
};
use models::error::AppError;
use queries::api_key_audit::{
    GetApiAuditAnalyticsQuery as GetApiAuditAnalyticsDataQuery,
    GetApiAuditLogsQuery as GetApiAuditLogsDataQuery,
    GetApiAuditTimeseriesQuery as GetApiAuditTimeseriesDataQuery,
};

use super::api_key_shared::get_api_auth_app_by_slug;

fn decode_cursor(cursor: String) -> Result<(chrono::DateTime<chrono::Utc>, String), AppError> {
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| AppError::BadRequest("invalid cursor".to_string()))?;
    let decoded = String::from_utf8(decoded)
        .map_err(|_| AppError::BadRequest("invalid cursor".to_string()))?;
    let parts: Vec<&str> = decoded.splitn(2, '|').collect();
    if parts.len() != 2 {
        return Err(AppError::BadRequest("invalid cursor".to_string()));
    }

    let cursor_ms: i64 = parts[0]
        .parse()
        .map_err(|_| AppError::BadRequest("invalid cursor".to_string()))?;

    let cursor_ts = chrono::DateTime::from_timestamp_millis(cursor_ms)
        .ok_or_else(|| AppError::BadRequest("invalid cursor".to_string()))?;

    Ok((cursor_ts, parts[1].to_string()))
}

pub async fn get_api_audit_logs(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
    params: ListApiAuditLogsQuery,
) -> Result<ApiAuditLogsResponse, AppError> {
    get_api_auth_app_by_slug(app_state, deployment_id, app_slug.clone()).await?;
    let clickhouse_client = &app_state.clickhouse_service.client;

    let mut cursor_ts = params.cursor_ts;
    let mut cursor_id = params.cursor_id.clone();

    if let Some(cursor) = params.cursor {
        let (decoded_ts, decoded_id) = decode_cursor(cursor)?;
        cursor_ts = Some(decoded_ts);
        cursor_id = Some(decoded_id);
    }

    GetApiAuditLogsDataQuery {
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
    .execute_with(clickhouse_client)
    .await
}

pub async fn get_api_audit_analytics(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
    params: GetApiAuditAnalyticsQuery,
) -> Result<ApiAuditAnalyticsResponse, AppError> {
    get_api_auth_app_by_slug(app_state, deployment_id, app_slug.clone()).await?;
    let clickhouse_client = &app_state.clickhouse_service.client;

    GetApiAuditAnalyticsDataQuery {
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
    .execute_with(clickhouse_client)
    .await
}

pub async fn get_api_audit_timeseries(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
    params: GetApiAuditTimeseriesQuery,
) -> Result<ApiAuditTimeseriesResponse, AppError> {
    get_api_auth_app_by_slug(app_state, deployment_id, app_slug.clone()).await?;
    let clickhouse_client = &app_state.clickhouse_service.client;

    let interval = params.interval.unwrap_or_else(|| "hour".to_string());
    let normalized_interval = match interval.as_str() {
        "minute" | "hour" | "day" | "week" | "month" => interval,
        _ => "hour".to_string(),
    };

    GetApiAuditTimeseriesDataQuery {
        deployment_id,
        app_slug,
        start_date: params.start_date,
        end_date: params.end_date,
        interval: normalized_interval,
        key_id: params.key_id.map(|v| v.get()),
    }
    .execute_with(clickhouse_client)
    .await
}
