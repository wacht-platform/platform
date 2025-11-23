use reqwest::{Client, header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue}};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::env;
use thiserror::Error;
use tracing::debug;

#[derive(Error, Debug)]
pub enum DodoError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Dodo API error: {0}")]
    ApiError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),
}

pub type DodoResult<T> = Result<T, DodoError>;

#[derive(Clone)]
pub struct DodoClient {
    client: Client,
    base_url: String,
    webhook_secret: String,
}

impl DodoClient {
    pub fn new() -> DodoResult<Self> {
        let api_key = env::var("DODO_API_KEY")
            .map_err(|_| DodoError::ConfigError("DODO_API_KEY not set".to_string()))?;

        let webhook_secret = env::var("DODO_WEBHOOK_SECRET").unwrap_or_default();

        let base_url = env::var("DODO_API_URL")
            .unwrap_or_else(|_| "https://live.dodopayments.com".to_string());

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", api_key))
                .map_err(|_| DodoError::ConfigError("Invalid API key format".to_string()))?,
        );
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        let client = Client::builder().default_headers(headers).build()?;

        Ok(Self {
            client,
            base_url,
            webhook_secret,
        })
    }

    // ==================== Checkout ====================

    pub async fn create_checkout_session(
        &self,
        params: CreateCheckoutParams,
    ) -> DodoResult<CheckoutSession> {
        let url = format!("{}/checkouts", self.base_url);
        debug!("Creating checkout session: {:?}", params);
        let response = self.client.post(&url).json(&params).send().await?;
        self.handle_response(response).await
    }

    // ==================== Subscriptions ====================

    pub async fn get_subscription(&self, subscription_id: &str) -> DodoResult<Subscription> {
        let url = format!("{}/subscriptions/{}", self.base_url, subscription_id);
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }

    pub async fn update_subscription(
        &self,
        subscription_id: &str,
        params: UpdateSubscriptionParams,
    ) -> DodoResult<Subscription> {
        let url = format!("{}/subscriptions/{}", self.base_url, subscription_id);
        let response = self.client.patch(&url).json(&params).send().await?;
        self.handle_response(response).await
    }

    pub async fn change_plan(
        &self,
        subscription_id: &str,
        params: ChangePlanParams,
    ) -> DodoResult<Subscription> {
        let url = format!("{}/subscriptions/{}/change-plan", self.base_url, subscription_id);
        let response = self.client.post(&url).json(&params).send().await?;
        self.handle_response(response).await
    }

    pub async fn cancel_subscription(
        &self,
        subscription_id: &str,
        status: &str,
    ) -> DodoResult<Subscription> {
        let url = format!("{}/subscriptions/{}", self.base_url, subscription_id);
        let params = serde_json::json!({ "status": status });
        let response = self.client.patch(&url).json(&params).send().await?;
        self.handle_response(response).await
    }

    pub async fn charge_subscription(&self, subscription_id: &str) -> DodoResult<JsonValue> {
        let url = format!("{}/subscriptions/{}/charge", self.base_url, subscription_id);
        let response = self.client.post(&url).send().await?;
        self.handle_response(response).await
    }

    pub async fn update_payment_method(
        &self,
        subscription_id: &str,
    ) -> DodoResult<PaymentMethodUpdateResponse> {
        let url = format!("{}/subscriptions/{}/update-payment-method", self.base_url, subscription_id);
        let response = self.client.post(&url).send().await?;
        self.handle_response(response).await
    }

    pub async fn get_usage_history(
        &self,
        subscription_id: &str,
    ) -> DodoResult<UsageHistoryResponse> {
        let url = format!("{}/subscriptions/{}/usage-history", self.base_url, subscription_id);
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }

    // ==================== Customers ====================

    pub async fn create_customer(&self, params: CreateCustomerParams) -> DodoResult<Customer> {
        let url = format!("{}/customers", self.base_url);
        let response = self.client.post(&url).json(&params).send().await?;
        self.handle_response(response).await
    }

    pub async fn get_customer(&self, customer_id: &str) -> DodoResult<Customer> {
        let url = format!("{}/customers/{}", self.base_url, customer_id);
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }

    pub async fn update_customer(
        &self,
        customer_id: &str,
        params: UpdateCustomerParams,
    ) -> DodoResult<Customer> {
        let url = format!("{}/customers/{}", self.base_url, customer_id);
        let response = self.client.patch(&url).json(&params).send().await?;
        self.handle_response(response).await
    }

    pub async fn create_portal_session(&self, customer_id: &str) -> DodoResult<PortalSession> {
        let url = format!("{}/customers/{}/customer-portal/session", self.base_url, customer_id);
        let response = self.client.post(&url).send().await?;
        self.handle_response(response).await
    }

    // ==================== Usage/Events ====================

    pub async fn ingest_usage_events(
        &self,
        customer_id: &str,
        event_name: &str,
        value: i64,
        event_id: &str,
        use_last_aggregation: bool,
    ) -> DodoResult<IngestEventsResponse> {
        let url = format!("{}/events/ingest", self.base_url);

        let metadata = if use_last_aggregation {
            serde_json::json!({ "value": value })
        } else {
            serde_json::json!({ "delta": value })
        };

        let payload = serde_json::json!({
            "events": [{
                "customer_id": customer_id,
                "event_id": event_id,
                "event_name": event_name,
                "metadata": metadata,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }]
        });

        debug!("Ingesting usage events: {:?}", payload);

        let response = self.client.post(&url).json(&payload).send().await?;
        self.handle_response(response).await
    }

    // ==================== Payments ====================

    pub async fn get_payment(&self, payment_id: &str) -> DodoResult<Payment> {
        let url = format!("{}/payments/{}", self.base_url, payment_id);
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }

    pub async fn list_payments(&self, customer_id: Option<&str>) -> DodoResult<ListPaymentsResponse> {
        let mut url = format!("{}/payments", self.base_url);

        if let Some(cid) = customer_id {
            url = format!("{}?customer_id={}", url, cid);
        }

        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }

    pub async fn get_payment_invoice(&self, payment_id: &str) -> DodoResult<Invoice> {
        let url = format!("{}/invoices/payments/{}", self.base_url, payment_id);
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }

    // ==================== Webhooks ====================

    pub fn verify_webhook(&self, payload: &str, signature: &str, timestamp: &str) -> bool {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        if self.webhook_secret.is_empty() {
            return false;
        }

        type HmacSha256 = Hmac<Sha256>;

        let signed_payload = format!("{}.{}", timestamp, payload);

        let mut mac = match HmacSha256::new_from_slice(self.webhook_secret.as_bytes()) {
            Ok(m) => m,
            Err(_) => return false,
        };

        mac.update(signed_payload.as_bytes());
        let result = mac.finalize();
        let expected = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            result.into_bytes(),
        );

        expected == signature
    }

    async fn handle_response<T: for<'de> Deserialize<'de>>(
        &self,
        response: reqwest::Response,
    ) -> DodoResult<T> {
        let status = response.status();
        let body = response.text().await?;

        if status.is_success() {
            serde_json::from_str(&body).map_err(|e| {
                DodoError::InvalidResponse(format!("Failed to parse response: {} - Body: {}", e, body))
            })
        } else {
            let error_msg = if let Ok(json) = serde_json::from_str::<JsonValue>(&body) {
                json.get("message")
                    .or_else(|| json.get("error"))
                    .and_then(|m| m.as_str())
                    .unwrap_or(&body)
                    .to_string()
            } else {
                body
            };

            Err(DodoError::ApiError(format!("Status {}: {}", status, error_msg)))
        }
    }
}

// ==================== Request/Response Types ====================

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateCheckoutParams {
    pub product_cart: Vec<ProductCartItem>,
    pub return_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer: Option<CheckoutCustomer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discount_code: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProductCartItem {
    pub product_id: String,
    pub quantity: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckoutCustomer {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckoutSession {
    pub checkout_id: String,
    pub checkout_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Subscription {
    pub subscription_id: String,
    pub customer_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_period_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_period_end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateSubscriptionParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChangePlanParams {
    pub product_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proration_mode: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PaymentMethodUpdateResponse {
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateCustomerParams {
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateCustomerParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Customer {
    pub customer_id: String,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PortalSession {
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IngestEventsResponse {
    pub ingested_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageHistoryResponse {
    pub items: Vec<UsageHistoryItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageHistoryItem {
    pub meter_id: String,
    pub total_value: i64,
    pub period_start: String,
    pub period_end: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Payment {
    pub payment_id: String,
    pub customer_id: String,
    pub amount: i64,
    pub currency: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListPaymentsResponse {
    pub items: Vec<Payment>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Invoice {
    pub invoice_url: Option<String>,
    pub invoice_pdf_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: JsonValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}
