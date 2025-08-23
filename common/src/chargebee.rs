use reqwest::{Client, header::{HeaderMap, HeaderValue, AUTHORIZATION}};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::env;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ChargebeeError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),
    
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    
    #[error("Chargebee API error: {0}")]
    ApiError(String),
    
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

pub type ChargebeeResult<T> = Result<T, ChargebeeError>;

#[derive(Clone)]
pub struct ChargebeeClient {
    client: Client,
    base_url: String,
    api_key: String,
}

impl ChargebeeClient {
    pub fn new() -> ChargebeeResult<Self> {
        let api_key = env::var("CHARGEBEE_API_KEY")
            .map_err(|_| ChargebeeError::ConfigError("CHARGEBEE_API_KEY not set".to_string()))?;
        
        let site = env::var("CHARGEBEE_SITE")
            .map_err(|_| ChargebeeError::ConfigError("CHARGEBEE_SITE not set".to_string()))?;
        
        let base_url = format!("https://{}.chargebee.com/api/v2", site);
        
        let mut headers = HeaderMap::new();
        use base64::{Engine as _, engine::general_purpose};
        let auth_value = format!("Basic {}", general_purpose::STANDARD.encode(format!("{}:", api_key)));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value)
                .map_err(|_| ChargebeeError::ConfigError("Invalid API key format".to_string()))?
        );
        
        let client = Client::builder()
            .default_headers(headers)
            .build()?;
        
        Ok(Self {
            client,
            base_url,
            api_key,
        })
    }
    
    // Subscription Management
    pub async fn create_subscription(&self, params: CreateSubscriptionParams) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/subscriptions", self.base_url);
        let response = self.client
            .post(&url)
            .json(&params)
            .send()
            .await?;
        
        self.handle_response(response).await
    }
    
    pub async fn get_subscription(&self, subscription_id: &str) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/subscriptions/{}", self.base_url, subscription_id);
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }
    
    pub async fn update_subscription(&self, subscription_id: &str, params: UpdateSubscriptionParams) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/subscriptions/{}", self.base_url, subscription_id);
        let response = self.client
            .post(&url)
            .json(&params)
            .send()
            .await?;
        
        self.handle_response(response).await
    }
    
    pub async fn cancel_subscription(&self, subscription_id: &str, end_of_term: bool) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/subscriptions/{}/cancel", self.base_url, subscription_id);
        let params = serde_json::json!({
            "end_of_term": end_of_term
        });
        
        let response = self.client
            .post(&url)
            .json(&params)
            .send()
            .await?;
        
        self.handle_response(response).await
    }
    
    // Customer Management
    pub async fn create_customer(&self, params: CreateCustomerParams) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/customers", self.base_url);
        let response = self.client
            .post(&url)
            .json(&params)
            .send()
            .await?;
        
        self.handle_response(response).await
    }
    
    pub async fn get_customer(&self, customer_id: &str) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/customers/{}", self.base_url, customer_id);
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }
    
    // Portal Session
    pub async fn create_portal_session(&self, customer_id: &str, redirect_url: Option<String>) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/portal_sessions", self.base_url);
        let mut params = serde_json::json!({
            "customer": {
                "id": customer_id
            }
        });
        
        if let Some(redirect) = redirect_url {
            params["redirect_url"] = serde_json::json!(redirect);
        }
        
        let response = self.client
            .post(&url)
            .json(&params)
            .send()
            .await?;
        
        self.handle_response(response).await
    }
    
    // Hosted Page for Checkout
    pub async fn create_checkout_session(&self, params: CreateCheckoutParams) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/hosted_pages/checkout_new", self.base_url);
        let response = self.client
            .post(&url)
            .json(&params)
            .send()
            .await?;
        
        self.handle_response(response).await
    }
    
    // Invoice Management
    pub async fn list_invoices(&self, subscription_id: Option<&str>, limit: Option<i32>) -> ChargebeeResult<JsonValue> {
        let mut url = format!("{}/invoices", self.base_url);
        let mut params = vec![];
        
        if let Some(sub_id) = subscription_id {
            params.push(format!("subscription_id[is]={}", sub_id));
        }
        
        if let Some(limit_val) = limit {
            params.push(format!("limit={}", limit_val));
        }
        
        if !params.is_empty() {
            url = format!("{}?{}", url, params.join("&"));
        }
        
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }
    
    pub async fn get_invoice(&self, invoice_id: &str) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/invoices/{}", self.base_url, invoice_id);
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }
    
    // Usage Recording
    pub async fn record_usage(&self, subscription_id: &str, usage: UsageRecord) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/subscriptions/{}/add_charge", self.base_url, subscription_id);
        let response = self.client
            .post(&url)
            .json(&usage)
            .send()
            .await?;
        
        self.handle_response(response).await
    }
    
    // Plans
    pub async fn list_plans(&self) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/plans", self.base_url);
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }
    
    pub async fn get_plan(&self, plan_id: &str) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/plans/{}", self.base_url, plan_id);
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }
    
    // Webhook signature verification
    pub fn verify_webhook_signature(&self, payload: &str, signature: &str) -> bool {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        
        type HmacSha256 = Hmac<Sha256>;
        
        let mut mac = HmacSha256::new_from_slice(self.api_key.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(payload.as_bytes());
        
        let result = mac.finalize();
        let expected = hex::encode(result.into_bytes());
        
        expected == signature
    }
    
    async fn handle_response(&self, response: reqwest::Response) -> ChargebeeResult<JsonValue> {
        let status = response.status();
        let body = response.text().await?;
        
        if status.is_success() {
            serde_json::from_str(&body)
                .map_err(|e| ChargebeeError::InvalidResponse(format!("Failed to parse response: {}", e)))
        } else {
            let error_msg = if let Ok(json) = serde_json::from_str::<JsonValue>(&body) {
                json.get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or(&body)
                    .to_string()
            } else {
                body
            };
            
            Err(ChargebeeError::ApiError(format!("Status {}: {}", status, error_msg)))
        }
    }
}

// Request/Response Types
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSubscriptionParams {
    pub plan_id: String,
    pub customer: CustomerInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trial_end: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CustomerInfo {
    pub id: Option<String>,
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub company: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateSubscriptionParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_quantity: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trial_end: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateCustomerParams {
    pub id: Option<String>,
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub company: Option<String>,
    pub phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateCheckoutParams {
    pub subscription: CheckoutSubscription,
    pub customer: CustomerInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckoutSubscription {
    pub plan_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trial_end: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageRecord {
    pub amount: i64, // in cents
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_from: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_to: Option<i64>,
}

// Webhook Event Types
#[derive(Debug, Deserialize)]
pub struct WebhookEvent {
    pub id: String,
    pub occurred_at: i64,
    pub source: String,
    pub event_type: String,
    pub content: JsonValue,
}