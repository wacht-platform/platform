use serde::{Deserialize, Serialize};
use crate::models::webhook::WebhookApp;

// Webhook status response for deployment
#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookStatus {
    pub is_activated: bool,
    pub app: Option<WebhookApp>,
    pub stats: Option<WebhookStats>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookStats {
    pub total_deliveries: i64,
    pub success_rate: f64,
    pub active_endpoints: i64,
    pub failed_deliveries_24h: i64,
}

// Webhook activation response
#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookActivationResponse {
    pub app: WebhookApp,
    pub message: String,
}

// Webhook endpoint test response
#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookEndpointTestResponse {
    pub success: bool,
    pub status_code: u16,
    pub response_time_ms: u32,
    pub response_body: Option<String>,
    pub error: Option<String>,
}