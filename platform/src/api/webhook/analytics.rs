use axum::extract::{Path, Query, State};
use common::state::AppState;
use dto::json::webhook_requests::{WebhookAnalyticsQuery, WebhookTimeseriesQuery};
use models::webhook_analytics::{WebhookAnalyticsResult, WebhookTimeseriesResult};

use crate::application::{response::ApiResult, webhook_analytics as webhook_analytics_app};
use crate::middleware::RequireDeployment;

pub async fn get_webhook_analytics(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<WebhookAnalyticsQuery>,
) -> ApiResult<WebhookAnalyticsResult> {
    let result =
        webhook_analytics_app::get_webhook_analytics(&app_state, deployment_id, app_slug, params)
            .await?;

    Ok(result.into())
}

pub async fn get_webhook_timeseries(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(app_slug): Path<String>,
    Query(params): Query<WebhookTimeseriesQuery>,
) -> ApiResult<WebhookTimeseriesResult> {
    let result =
        webhook_analytics_app::get_webhook_timeseries(&app_state, deployment_id, app_slug, params)
            .await?;

    Ok(result.into())
}
