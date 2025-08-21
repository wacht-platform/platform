use chrono::{DateTime, Utc};
use serde::Deserialize;

use common::error::AppError;
use common::state::AppState;
use models::webhook_analytics::{
    EndpointPerformance, EventCount, FailureReason, TimeseriesInterval, TimeseriesPoint,
    WebhookAnalyticsResult, WebhookTimeseriesResult,
};

use super::Query;

#[derive(Debug, Deserialize)]
pub struct GetWebhookAnalyticsQuery {
    pub deployment_id: i64,
    pub app_name: Option<String>,
    pub endpoint_id: Option<i64>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
}

impl GetWebhookAnalyticsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            app_name: None,
            endpoint_id: None,
            start_date: None,
            end_date: None,
        }
    }

    pub fn with_app_name(mut self, app_name: String) -> Self {
        self.app_name = Some(app_name);
        self
    }

    pub fn with_endpoint(mut self, endpoint_id: i64) -> Self {
        self.endpoint_id = Some(endpoint_id);
        self
    }

    pub fn with_date_range(mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        self.start_date = Some(start);
        self.end_date = Some(end);
        self
    }
}

impl Query for GetWebhookAnalyticsQuery {
    type Output = WebhookAnalyticsResult;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let start_date = self
            .start_date
            .unwrap_or_else(|| Utc::now() - chrono::Duration::days(30));
        let end_date = self.end_date.unwrap_or_else(|| Utc::now());

        // Get delivery statistics
        let stats = app_state
            .clickhouse_service
            .get_webhook_delivery_stats(
                self.deployment_id,
                self.app_name.clone(),
                self.endpoint_id,
                start_date,
                end_date,
            )
            .await?;

        // Get event distribution (top 10 events)
        let event_distribution = app_state
            .clickhouse_service
            .get_webhook_event_distribution(
                self.deployment_id,
                self.app_name.clone(),
                start_date,
                end_date,
                10,
            )
            .await?;

        let top_events: Vec<EventCount> = event_distribution
            .into_iter()
            .map(|e| EventCount {
                event_name: e.event_name,
                count: e.count,
            })
            .collect();

        // Get endpoint performance
        let endpoint_perf_data = if let Some(endpoint_id) = self.endpoint_id {
            // Get specific endpoint performance
            let perf = app_state
                .clickhouse_service
                .get_webhook_endpoint_performance(
                    self.deployment_id,
                    endpoint_id,
                    start_date,
                    end_date,
                )
                .await?;
            vec![EndpointPerformance {
                endpoint_id,
                endpoint_url: perf.endpoint_url,
                total_attempts: perf.total_attempts,
                successful_attempts: perf.successful_attempts,
                failed_attempts: perf.total_attempts - perf.successful_attempts,
                avg_response_time_ms: perf.avg_response_time_ms,
                success_rate: perf.success_rate,
            }]
        } else if let Some(ref app_name) = self.app_name {
            // Get all endpoints for the app
            let perf_data = app_state
                .clickhouse_service
                .get_app_endpoints_performance(
                    self.deployment_id,
                    app_name.clone(),
                    start_date,
                    end_date,
                )
                .await?;
            perf_data
                .into_iter()
                .map(|p| EndpointPerformance {
                    endpoint_id: p.endpoint_id,
                    endpoint_url: p.endpoint_url,
                    total_attempts: p.total_attempts,
                    successful_attempts: p.successful_attempts,
                    failed_attempts: p.failed_attempts,
                    avg_response_time_ms: p.avg_response_time_ms,
                    success_rate: p.success_rate,
                })
                .collect()
        } else {
            Vec::new()
        };

        // Get failure reasons
        let service_failure_reasons = app_state
            .clickhouse_service
            .get_webhook_failure_reasons(
                self.deployment_id,
                self.app_name.clone(),
                self.endpoint_id,
                start_date,
                end_date,
            )
            .await?;

        let failure_reasons: Vec<FailureReason> = service_failure_reasons
            .into_iter()
            .map(|f| FailureReason {
                reason: f.reason,
                count: f.count,
            })
            .collect();

        let success_rate = if stats.total_deliveries > 0 {
            (stats.successful_deliveries as f64 / stats.total_deliveries as f64) * 100.0
        } else {
            0.0
        };

        Ok(WebhookAnalyticsResult {
            total_events: stats.total_events,
            total_deliveries: stats.total_deliveries,
            successful_deliveries: stats.successful_deliveries,
            failed_deliveries: stats.failed_deliveries,
            filtered_deliveries: stats.filtered_deliveries,
            avg_response_time_ms: stats.avg_response_time_ms,
            p50_response_time_ms: stats.p50_response_time_ms,
            p95_response_time_ms: stats.p95_response_time_ms,
            p99_response_time_ms: stats.p99_response_time_ms,
            success_rate,
            top_events,
            endpoint_performance: endpoint_perf_data,
            failure_reasons,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct GetWebhookTimeseriesQuery {
    pub deployment_id: i64,
    pub app_name: Option<String>,
    pub endpoint_id: Option<i64>,
    pub interval: TimeseriesInterval,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
}

impl GetWebhookTimeseriesQuery {
    pub fn new(deployment_id: i64, interval: TimeseriesInterval) -> Self {
        Self {
            deployment_id,
            app_name: None,
            endpoint_id: None,
            interval,
            start_date: None,
            end_date: None,
        }
    }

    pub fn with_app_name(mut self, app_name: String) -> Self {
        self.app_name = Some(app_name);
        self
    }

    pub fn with_endpoint(mut self, endpoint_id: i64) -> Self {
        self.endpoint_id = Some(endpoint_id);
        self
    }

    pub fn with_date_range(mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        self.start_date = Some(start);
        self.end_date = Some(end);
        self
    }
}

impl Query for GetWebhookTimeseriesQuery {
    type Output = WebhookTimeseriesResult;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let start_date = self
            .start_date
            .unwrap_or_else(|| Utc::now() - chrono::Duration::days(7));
        let end_date = self.end_date.unwrap_or_else(|| Utc::now());

        let timeseries_data = app_state
            .clickhouse_service
            .get_webhook_timeseries(
                self.deployment_id,
                self.app_name.clone(),
                self.endpoint_id,
                &self.interval,
                start_date,
                end_date,
            )
            .await?;

        let data: Vec<TimeseriesPoint> = timeseries_data
            .into_iter()
            .map(|d| TimeseriesPoint {
                timestamp: d.timestamp,
                total_events: d.total_events,
                total_deliveries: d.total_deliveries,
                successful_deliveries: d.successful_deliveries,
                failed_deliveries: d.failed_deliveries,
                filtered_deliveries: d.filtered_deliveries,
                avg_response_time_ms: d.avg_response_time_ms,
                success_rate: d.success_rate,
            })
            .collect();

        Ok(WebhookTimeseriesResult {
            data,
            interval: format!("{:?}", self.interval).to_lowercase(),
        })
    }
}
