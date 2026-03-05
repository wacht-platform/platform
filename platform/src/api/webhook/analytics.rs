use axum::extract::{Path, Query, State};
use common::state::AppState;
use dto::json::webhook_requests::{WebhookAnalyticsQuery, WebhookTimeseriesQuery};
use models::webhook_analytics::{WebhookAnalyticsResult, WebhookTimeseriesResult};
use queries::webhook_analytics::{GetWebhookAnalyticsQuery, GetWebhookTimeseriesQuery};
use queries::Query as QueryTrait;

use crate::application::response::ApiResult;
use crate::middleware::RequireDeployment;

pub async fn get_webhook_analytics(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<WebhookAnalyticsQuery>,
) -> ApiResult<WebhookAnalyticsResult> {
    let mut query = GetWebhookAnalyticsQuery::new(deployment_id).with_app_slug(app_slug);

    if let Some(endpoint_id) = params.endpoint_id {
        query = query.with_endpoint(endpoint_id);
    }

    if let (Some(start), Some(end)) = (params.start_date, params.end_date) {
        query = query.with_date_range(start, end);
    }

    let result = query.execute(&app_state).await?;

    Ok(result.into())
}

pub async fn get_webhook_timeseries(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<WebhookTimeseriesQuery>,
) -> ApiResult<WebhookTimeseriesResult> {
    let mut query =
        GetWebhookTimeseriesQuery::new(deployment_id, params.interval).with_app_slug(app_slug);

    if let Some(endpoint_id) = params.endpoint_id {
        query = query.with_endpoint(endpoint_id);
    }

    if let (Some(start), Some(end)) = (params.start_date, params.end_date) {
        query = query.with_date_range(start, end);
    }

    let result = query.execute(&app_state).await?;

    Ok(result.into())
}
