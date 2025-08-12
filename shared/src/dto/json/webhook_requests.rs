use serde::{Deserialize, Serialize};
use serde_json::Value;
use chrono::{DateTime, Utc};
use crate::models::webhook::WebhookEventDefinition;
use crate::commands::webhook_trigger::WebhookEventTrigger;

// =====================================================
// WEBHOOK APP REQUESTS
// =====================================================

#[derive(Debug, Deserialize)]
pub struct ListWebhookAppsQuery {
    pub include_inactive: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ListWebhookAppsResponse {
    pub apps: Vec<crate::models::webhook::WebhookApp>,
    pub total: usize,
}

#[derive(Debug, Deserialize)]
pub struct CreateWebhookAppRequest {
    pub name: String,
    pub description: Option<String>,
    pub events: Vec<WebhookEventDefinition>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateWebhookAppRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
}

// =====================================================
// WEBHOOK ENDPOINT REQUESTS
// =====================================================

#[derive(Debug, Deserialize)]
pub struct ListWebhookEndpointsQuery {
    pub app_name: Option<String>,
    pub include_inactive: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ListWebhookEndpointsResponse {
    pub endpoints: Vec<crate::models::webhook::WebhookEndpoint>,
    pub total: usize,
}

#[derive(Debug, Deserialize)]
pub struct EventSubscription {
    pub event_name: String,
    pub filter_rules: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWebhookEndpointRequest {
    pub app_name: String,
    pub url: String,
    pub description: Option<String>,
    pub subscriptions: Vec<EventSubscription>,
    pub headers: Option<Value>,
    pub max_retries: Option<i32>,
    pub timeout_seconds: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateWebhookEndpointRequest {
    pub url: Option<String>,
    pub description: Option<String>,
    pub headers: Option<Value>,
    pub max_retries: Option<i32>,
    pub timeout_seconds: Option<i32>,
    pub is_active: Option<bool>,
}

// =====================================================
// WEBHOOK TRIGGER REQUESTS
// =====================================================

#[derive(Debug, Deserialize)]
pub struct TriggerWebhookEventRequest {
    pub app_name: String,
    pub event_name: String,
    pub payload: Value,
    pub filter_context: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct TriggerWebhookEventResponse {
    pub delivery_ids: Vec<i64>,
    pub filtered_count: usize,
    pub delivered_count: usize,
}

#[derive(Debug, Deserialize)]
pub struct BatchTriggerWebhookEventsRequest {
    pub app_name: String,
    pub events: Vec<WebhookEventTrigger>,
}

// =====================================================
// WEBHOOK DELIVERY REQUESTS
// =====================================================

#[derive(Debug, Deserialize)]
pub struct ReplayWebhookDeliveryRequest {
    pub delivery_id: i64,
}

#[derive(Debug, Serialize)]
pub struct ReplayWebhookDeliveryResponse {
    pub new_delivery_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct GetWebhookDeliveryStatusRequest {
    pub delivery_id: i64,
}

#[derive(Debug, Serialize)]
pub struct WebhookDeliveryStatus {
    pub id: i64,
    pub endpoint_id: i64,
    pub event_name: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub status: String,
}

// =====================================================
// WEBHOOK ENDPOINT MANAGEMENT REQUESTS
// =====================================================

#[derive(Debug, Deserialize)]
pub struct ReactivateEndpointRequest {
    pub endpoint_id: i64,
}

#[derive(Debug, Serialize)]
pub struct ReactivateEndpointResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct TestWebhookEndpointRequest {
    pub endpoint_id: i64,
    pub event_name: String,
    pub payload: Value,
}

#[derive(Debug, Serialize)]
pub struct TestWebhookEndpointResponse {
    pub success: bool,
    pub status_code: u16,
    pub response_time_ms: u32,
    pub response_body: Option<String>,
    pub error: Option<String>,
}

// =====================================================
// WEBHOOK ANALYTICS REQUESTS
// =====================================================

#[derive(Debug, Deserialize)]
pub struct WebhookAnalyticsQuery {
    pub app_id: Option<i64>,
    pub endpoint_id: Option<i64>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookTimeseriesQuery {
    pub app_id: Option<i64>,
    pub endpoint_id: Option<i64>,
    #[serde(default = "default_interval")]
    pub interval: crate::queries::webhook_analytics::TimeseriesInterval,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
}

fn default_interval() -> crate::queries::webhook_analytics::TimeseriesInterval {
    crate::queries::webhook_analytics::TimeseriesInterval::Day
}

// =====================================================
// WEBHOOK CONSOLE REQUESTS
// =====================================================

#[derive(Debug, Deserialize)]
pub struct ConsoleAnalyticsQuery {
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct ConsoleTimeseriesQuery {
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    #[serde(default = "default_interval_string")]
    pub interval: String,
}

fn default_interval_string() -> String {
    "hour".to_string()
}

#[derive(Debug, Deserialize)]
pub struct DeliveryListQuery {
    pub status: Option<String>,
    pub event_name: Option<String>,
    pub limit: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct WebhookDeliveryItem {
    pub delivery_id: i64,
    pub app_id: i64,
    pub app_name: String,
    pub endpoint_id: i64,
    pub endpoint_url: String,
    pub event_name: String,
    pub status: String,
    pub http_status_code: Option<i32>,
    pub response_time_ms: Option<i32>,
    pub attempt_number: i32,
    pub error_message: Option<String>,
    pub filtered_reason: Option<String>,
    pub timestamp: DateTime<Utc>,
}