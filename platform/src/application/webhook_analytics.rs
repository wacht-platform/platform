use common::error::AppError;
use common::state::AppState;
use dto::json::webhook_requests::{WebhookAnalyticsQuery, WebhookTimeseriesQuery};
use models::webhook_analytics::{WebhookAnalyticsResult, WebhookTimeseriesResult};
use queries::Query as QueryTrait;
use queries::webhook_analytics::{GetWebhookAnalyticsQuery, GetWebhookTimeseriesQuery};

pub async fn get_webhook_analytics(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
    params: WebhookAnalyticsQuery,
) -> Result<WebhookAnalyticsResult, AppError> {
    let mut query = GetWebhookAnalyticsQuery::new(deployment_id).with_app_slug(app_slug);

    if let Some(endpoint_id) = params.endpoint_id {
        query = query.with_endpoint(endpoint_id);
    }

    if let (Some(start), Some(end)) = (params.start_date, params.end_date) {
        query = query.with_date_range(start, end);
    }

    QueryTrait::execute(&query, app_state).await
}

pub async fn get_webhook_timeseries(
    app_state: &AppState,
    deployment_id: i64,
    app_slug: String,
    params: WebhookTimeseriesQuery,
) -> Result<WebhookTimeseriesResult, AppError> {
    let mut query =
        GetWebhookTimeseriesQuery::new(deployment_id, params.interval).with_app_slug(app_slug);

    if let Some(endpoint_id) = params.endpoint_id {
        query = query.with_endpoint(endpoint_id);
    }

    if let (Some(start), Some(end)) = (params.start_date, params.end_date) {
        query = query.with_date_range(start, end);
    }

    QueryTrait::execute(&query, app_state).await
}
