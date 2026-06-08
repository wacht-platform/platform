use axum::extract::{Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::application::analytics::{
    AnalyticsStatsResponse, get_analytics_stats as run_get_analytics_stats,
};
use crate::application::response::{ApiErrorResponse, ApiResult};
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
) -> ApiResult<AnalyticsStatsResponse> {
    let stats = run_get_analytics_stats(&app_state, deployment_id, query.from, query.to)
        .await
        .map_err(|status| ApiErrorResponse::from((status, "Failed to get analytics stats")))?;
    Ok(stats.into())
}

#[derive(Debug, Serialize)]
pub struct TokenUsageBucketJson {
    pub bucket: String,
    pub input_tokens: i64,
    pub cached_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub request_count: u64,
}

#[derive(Debug, Serialize)]
pub struct TokenUsageResponse {
    pub buckets: Vec<TokenUsageBucketJson>,
}

#[derive(Debug, Deserialize)]
pub struct TokenUsageQuery {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    #[serde(default)]
    pub granularity: Option<String>,
    #[serde(default)]
    pub tz: Option<String>,
}

#[instrument(skip(app_state))]
pub async fn get_token_usage_stats(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<TokenUsageQuery>,
) -> ApiResult<TokenUsageResponse> {
    let granularity = query.granularity.as_deref().unwrap_or("minute");
    let tz = query.tz.as_deref().unwrap_or("UTC");
    let buckets = app_state
        .clickhouse_service
        .get_deployment_token_usage(deployment_id, query.from, query.to, granularity, tz)
        .await
        .map_err(|_| {
            ApiErrorResponse::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to get token usage",
            ))
        })?
        .into_iter()
        .map(|b| TokenUsageBucketJson {
            bucket: b.bucket.to_rfc3339(),
            input_tokens: b.input_tokens,
            cached_tokens: b.cached_tokens,
            output_tokens: b.output_tokens,
            total_tokens: b.total_tokens,
            request_count: b.request_count,
        })
        .collect();
    Ok(TokenUsageResponse { buckets }.into())
}

#[derive(Debug, Serialize)]
pub struct WebhookUsageBucketJson {
    pub bucket: String,
    pub total_deliveries: i64,
    pub successful_deliveries: i64,
    pub failed_deliveries: i64,
    pub filtered_deliveries: i64,
    pub success_rate: f64,
}

#[derive(Debug, Serialize)]
pub struct WebhookUsageResponse {
    pub buckets: Vec<WebhookUsageBucketJson>,
}

#[instrument(skip(app_state))]
pub async fn get_webhook_usage_stats(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<TokenUsageQuery>,
) -> ApiResult<WebhookUsageResponse> {
    let granularity = query.granularity.as_deref().unwrap_or("minute");
    let tz = query.tz.as_deref().unwrap_or("UTC");
    let buckets = app_state
        .clickhouse_service
        .get_deployment_webhook_usage(deployment_id, query.from, query.to, granularity, tz)
        .await
        .map_err(|_| {
            ApiErrorResponse::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to get webhook usage",
            ))
        })?
        .into_iter()
        .map(|b| {
            let success_rate = if b.total_deliveries > 0 {
                (b.successful_deliveries as f64 / b.total_deliveries as f64) * 100.0
            } else {
                0.0
            };
            WebhookUsageBucketJson {
                bucket: b.bucket.to_rfc3339(),
                total_deliveries: b.total_deliveries,
                successful_deliveries: b.successful_deliveries,
                failed_deliveries: b.failed_deliveries,
                filtered_deliveries: b.filtered_deliveries,
                success_rate,
            }
        })
        .collect();
    Ok(WebhookUsageResponse { buckets }.into())
}

#[derive(Debug, Serialize)]
pub struct GatewayUsageBucketJson {
    pub bucket: String,
    pub total_requests: i64,
    pub allowed_requests: i64,
    pub blocked_requests: i64,
}

#[derive(Debug, Serialize)]
pub struct GatewayUsageResponse {
    pub buckets: Vec<GatewayUsageBucketJson>,
}

#[instrument(skip(app_state))]
pub async fn get_gateway_usage_stats(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<TokenUsageQuery>,
) -> ApiResult<GatewayUsageResponse> {
    let granularity = query.granularity.as_deref().unwrap_or("minute");
    let tz = query.tz.as_deref().unwrap_or("UTC");
    let buckets = app_state
        .clickhouse_service
        .get_deployment_gateway_usage(deployment_id, query.from, query.to, granularity, tz)
        .await
        .map_err(|_| {
            ApiErrorResponse::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to get gateway usage",
            ))
        })?
        .into_iter()
        .map(|b| GatewayUsageBucketJson {
            bucket: b.bucket.to_rfc3339(),
            total_requests: b.total_requests,
            allowed_requests: b.allowed_requests,
            blocked_requests: b.blocked_requests,
        })
        .collect();
    Ok(GatewayUsageResponse { buckets }.into())
}

#[derive(Debug, Serialize)]
pub struct TokenUsageByModelJson {
    pub model: String,
    pub input_tokens: i64,
    pub cached_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub request_count: u64,
}

#[derive(Debug, Serialize)]
pub struct TokenUsageByModelResponse {
    pub models: Vec<TokenUsageByModelJson>,
}

#[instrument(skip(app_state))]
pub async fn get_token_usage_by_model(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<TokenUsageQuery>,
) -> ApiResult<TokenUsageByModelResponse> {
    let models = app_state
        .clickhouse_service
        .get_deployment_token_usage_by_model(deployment_id, query.from, query.to)
        .await
        .map_err(|_| {
            ApiErrorResponse::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to get token usage by model",
            ))
        })?
        .into_iter()
        .map(|m| TokenUsageByModelJson {
            model: m.model,
            input_tokens: m.input_tokens,
            cached_tokens: m.cached_tokens,
            output_tokens: m.output_tokens,
            total_tokens: m.total_tokens,
            request_count: m.request_count,
        })
        .collect();
    Ok(TokenUsageByModelResponse { models }.into())
}
