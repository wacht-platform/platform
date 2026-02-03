use chrono::{DateTime, Utc};
use models::webhook::WebhookEventDefinition;
use models::webhook::WebhookEventTrigger;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// =====================================================
// WEBHOOK APP REQUESTS
// =====================================================

#[derive(Debug, Deserialize)]
pub struct ListWebhookAppsQuery {
    pub include_inactive: Option<bool>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
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

// Get available events response
#[derive(Debug, Serialize)]
pub struct GetAvailableEventsResponse {
    pub events: Vec<wacht::api::webhooks::WebhookAppEvent>,
}

// =====================================================
// WEBHOOK ENDPOINT REQUESTS
// =====================================================

#[derive(Debug, Deserialize)]
pub struct ListWebhookEndpointsQuery {
    pub include_inactive: Option<bool>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct WebhookEndpoint {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub app_name: String,
    pub url: String,
    pub description: Option<String>,
    pub headers: Option<Value>,
    pub is_active: bool,
    pub signing_secret: Option<String>,
    pub max_retries: i32,
    pub timeout_seconds: i32,
    pub failure_count: i32,
    pub last_failure_at: Option<chrono::DateTime<chrono::Utc>>,
    pub auto_disabled: bool,
    pub auto_disabled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub subscriptions: Vec<WebhookEndpointSubscription>,
}

#[derive(Debug, Serialize)]
pub struct WebhookEndpointSubscription {
    pub event_name: String,
    pub filter_rules: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct ListWebhookEndpointsResponse {
    pub endpoints: Vec<WebhookEndpoint>,
    pub count: usize, // Number of items in this response
    pub limit: i32,
    pub offset: i32,
    pub has_more: bool,
}

#[derive(Debug, Deserialize)]
pub struct EventSubscription {
    pub event_name: String,
    pub filter_rules: Option<Value>,
}

impl From<EventSubscription> for wacht::api::webhooks::EventSubscription {
    fn from(s: EventSubscription) -> Self {
        Self {
            event_name: s.event_name,
            filter_rules: s.filter_rules,
        }
    }
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

// Console API version - doesn't require app_name since it's derived from deployment_id
#[derive(Debug, Deserialize)]
pub struct CreateWebhookEndpointConsoleRequest {
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
    pub subscriptions: Option<Vec<EventSubscription>>,
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
#[serde(untagged)]
pub enum ReplayWebhookDeliveryRequest {
    ByIds {
        delivery_ids: Vec<String>,
        #[serde(default = "default_include_successful")]
        include_successful: bool,
    },
    ByDateRange {
        start_date: DateTime<Utc>,
        end_date: Option<DateTime<Utc>>,
        #[serde(default = "default_include_successful")]
        include_successful: bool,
    },
}

fn default_include_successful() -> bool {
    false // Default to excluding successful deliveries
}

#[derive(Debug, Serialize)]
pub struct ReplayWebhookDeliveryResponse {
    pub status: String,
    pub message: String,
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
pub struct TestWebhookRequest {
    pub event_name: String,
    pub payload: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct TestWebhookEndpointRequest {
    pub event_name: String,
    pub payload: Option<Value>,
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
    pub interval: models::webhook_analytics::TimeseriesInterval,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
}

fn default_interval() -> models::webhook_analytics::TimeseriesInterval {
    models::webhook_analytics::TimeseriesInterval::Day
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
    pub offset: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct WebhookDeliveryItem {
    pub delivery_id: i64,
    pub deployment_id: i64,
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

// =====================================================
// BACKEND API DELIVERY REQUESTS
// =====================================================

#[derive(Debug, Deserialize)]
pub struct GetWebhookDeliveriesQuery {
    pub app_name: Option<String>,
    pub endpoint_id: Option<i64>,
    pub event_name: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
}

// For app-specific deliveries endpoint where app_name comes from path
#[derive(Debug, Deserialize)]
pub struct GetAppWebhookDeliveriesQuery {
    pub endpoint_id: Option<i64>,
    pub event_name: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookDeliveryResponse {
    pub deployment_id: String,
    pub delivery_id: String,
    pub app_name: String,
    pub endpoint_id: String,
    pub endpoint_url: String,
    pub event_name: String,
    pub status: String,
    pub http_status_code: Option<i32>,
    pub response_time_ms: Option<i32>,
    pub attempt_number: i32,
    pub max_attempts: i32,
    pub error_message: Option<String>,
    pub filtered_reason: Option<String>,
    pub payload_s3_key: String,
    pub response_body: Option<String>,
    pub response_headers: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct GetWebhookDeliveriesResponse {
    pub deliveries: Vec<crate::clickhouse::webhook::WebhookDeliveryListResponse>,
    pub count: usize, // Number of items in this response
    pub limit: i32,
    pub offset: i32,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookDeliveryDetails {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub delivery_id: i64,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub app_name: String,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub endpoint_id: i64,
    pub endpoint_url: String,
    pub event_name: String,
    pub status: String,
    pub http_status_code: Option<i32>,
    pub response_time_ms: Option<i32>,
    pub attempt_number: i32,
    pub max_attempts: i32,
    pub error_message: Option<String>,
    pub filtered_reason: Option<String>,
    pub payload_s3_key: String,
    pub response_body: Option<String>,
    pub response_headers: Option<Value>,
    pub timestamp: DateTime<Utc>,
    pub payload: Option<Value>,
}
