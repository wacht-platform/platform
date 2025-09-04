use reqwest::{
    Client,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::env;
use thiserror::Error;
use tracing::debug;

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

        let base_url = format!("https://{}/api/v2", site);

        let mut headers = HeaderMap::new();
        use base64::{Engine as _, engine::general_purpose};
        let auth_value = format!(
            "Basic {}",
            general_purpose::STANDARD.encode(format!("{}:", api_key))
        );
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value)
                .map_err(|_| ChargebeeError::ConfigError("Invalid API key format".to_string()))?,
        );

        let client = Client::builder().default_headers(headers).build()?;

        Ok(Self {
            client,
            base_url,
            api_key,
        })
    }

    // Subscription Management
    pub async fn create_subscription(
        &self,
        params: CreateSubscriptionParams,
    ) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/subscriptions", self.base_url);
        let response = self.client.post(&url).json(&params).send().await?;

        self.handle_response(response).await
    }

    pub async fn get_subscription(&self, subscription_id: &str) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/subscriptions/{}", self.base_url, subscription_id);
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }

    pub async fn update_subscription(
        &self,
        subscription_id: &str,
        params: UpdateSubscriptionParams,
    ) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/subscriptions/{}", self.base_url, subscription_id);
        let response = self.client.post(&url).json(&params).send().await?;

        self.handle_response(response).await
    }

    pub async fn cancel_subscription(
        &self,
        subscription_id: &str,
        end_of_term: bool,
    ) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/subscriptions/{}/cancel", self.base_url, subscription_id);
        let params = serde_json::json!({
            "end_of_term": end_of_term
        });

        let response = self.client.post(&url).json(&params).send().await?;

        self.handle_response(response).await
    }

    // Customer Management
    pub async fn create_customer(
        &self,
        params: CreateCustomerParams,
    ) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/customers", self.base_url);
        let response = self.client.post(&url).json(&params).send().await?;

        self.handle_response(response).await
    }

    pub async fn get_customer(&self, customer_id: &str) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/customers/{}", self.base_url, customer_id);
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }

    // Portal Session
    pub async fn create_portal_session(
        &self,
        customer_id: &str,
        redirect_url: Option<String>,
    ) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/portal_sessions", self.base_url);
        let mut params = serde_json::json!({
            "customer": {
                "id": customer_id
            }
        });

        if let Some(redirect) = redirect_url {
            params["redirect_url"] = serde_json::json!(redirect);
        }

        let response = self.client.post(&url).json(&params).send().await?;

        self.handle_response(response).await
    }

    // Hosted Page for Checkout
    pub async fn create_checkout_session(
        &self,
        params: CreateCheckoutParams,
    ) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/hosted_pages/checkout_new_for_items", self.base_url);

        // Build form data
        let mut form_data = vec![];

        // Add subscription items
        for (i, item) in params.subscription_items.iter().enumerate() {
            form_data.push((
                format!("subscription_items[item_price_id][{}]", i),
                item.item_price_id.clone(),
            ));
            if let Some(quantity) = item.quantity {
                form_data.push((
                    format!("subscription_items[quantity][{}]", i),
                    quantity.to_string(),
                ));
            }
        }

        // Add customer info
        if let Some(id) = &params.customer.id {
            form_data.push(("customer[id]".to_string(), id.clone()));
        }
        form_data.push(("customer[email]".to_string(), params.customer.email.clone()));
        if let Some(first_name) = &params.customer.first_name {
            form_data.push(("customer[first_name]".to_string(), first_name.clone()));
        }
        if let Some(last_name) = &params.customer.last_name {
            form_data.push(("customer[last_name]".to_string(), last_name.clone()));
        }
        if let Some(company) = &params.customer.company {
            form_data.push(("customer[company]".to_string(), company.clone()));
        }
        if let Some(phone) = &params.customer.phone {
            form_data.push(("customer[phone]".to_string(), phone.clone()));
        }

        // Add billing address
        if let Some(billing_address) = &params.customer.billing_address {
            form_data.push((
                "billing_address[line1]".to_string(),
                billing_address.line1.clone(),
            ));
            if let Some(line2) = &billing_address.line2 {
                form_data.push(("billing_address[line2]".to_string(), line2.clone()));
            }
            form_data.push((
                "billing_address[city]".to_string(),
                billing_address.city.clone(),
            ));
            if let Some(state) = &billing_address.state {
                form_data.push(("billing_address[state]".to_string(), state.clone()));
            }
            form_data.push((
                "billing_address[zip]".to_string(),
                billing_address.zip.clone(),
            ));
            form_data.push((
                "billing_address[country]".to_string(),
                billing_address.country.clone(),
            ));
        }

        debug!("Creating checkout session with form data: {:?}", form_data);

        let response = self.client.post(&url).form(&form_data).send().await?;

        self.handle_response(response).await
    }

    // Invoice Management
    pub async fn list_invoices(
        &self,
        subscription_id: Option<&str>,
        limit: Option<i32>,
    ) -> ChargebeeResult<JsonValue> {
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

    // Usage Recording for metered billing
    pub async fn record_usage(
        &self,
        subscription_id: &str,
        item_price_id: &str,
        quantity: i64,
        usage_date: Option<i64>,
    ) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/usages", self.base_url);

        let mut form_data = vec![
            ("subscription_id".to_string(), subscription_id.to_string()),
            ("item_price_id".to_string(), item_price_id.to_string()),
            ("quantity".to_string(), quantity.to_string()),
        ];

        if let Some(date) = usage_date {
            form_data.push(("usage_date".to_string(), date.to_string()));
        } else {
            let now = chrono::Utc::now().timestamp();
            form_data.push(("usage_date".to_string(), now.to_string()));
        }

        let response = self.client.post(&url).form(&form_data).send().await?;

        self.handle_response(response).await
    }

    // Items (Product Catalog 2.0)
    pub async fn create_item(&self, params: CreateItemParams) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/items", self.base_url);
        let response = self.client.post(&url).json(&params).send().await?;
        self.handle_response(response).await
    }

    pub async fn list_items(&self) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/items", self.base_url);
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }

    pub async fn get_item(&self, item_id: &str) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/items/{}", self.base_url, item_id);
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }

    // Item Prices (Product Catalog 2.0)
    pub async fn create_item_price(
        &self,
        params: CreateItemPriceParams,
    ) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/item_prices", self.base_url);
        let response = self.client.post(&url).json(&params).send().await?;
        self.handle_response(response).await
    }

    pub async fn list_item_prices(&self) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/item_prices", self.base_url);
        let response = self.client.get(&url).send().await?;
        self.handle_response(response).await
    }

    pub async fn get_item_price(&self, item_price_id: &str) -> ChargebeeResult<JsonValue> {
        let url = format!("{}/item_prices/{}", self.base_url, item_price_id);
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
            serde_json::from_str(&body).map_err(|e| {
                ChargebeeError::InvalidResponse(format!("Failed to parse response: {}", e))
            })
        } else {
            let error_msg = if let Ok(json) = serde_json::from_str::<JsonValue>(&body) {
                json.get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or(&body)
                    .to_string()
            } else {
                body
            };

            Err(ChargebeeError::ApiError(format!(
                "Status {}: {}",
                status, error_msg
            )))
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
    pub phone: Option<String>,
    pub billing_address: Option<BillingAddress>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BillingAddress {
    pub line1: String,
    pub line2: Option<String>,
    pub city: String,
    pub state: Option<String>,
    pub zip: String,
    pub country: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateSubscriptionParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_quantity: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trial_end: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invoice_immediately: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invoice_immediately_min_amount: Option<i64>, // In cents (e.g., 5000 = $50)
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
    pub subscription_items: Vec<SubscriptionItem>,
    pub customer: CustomerInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubscriptionItem {
    pub item_price_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckoutSubscription {
    pub plan_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trial_end: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateItemParams {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub item_type: String, // "plan" or "addon" or "charge"
    pub status: Option<String>, // "active" or "archived"
    pub item_family_id: Option<String>,
    pub metadata: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateItemPriceParams {
    pub id: String,
    pub item_id: String,
    pub name: String,
    pub description: Option<String>,
    pub price_type: String, // "tax_exclusive", "tax_inclusive", or "tax_exempt"
    pub price: Option<i64>, // in cents
    pub period: Option<i32>, // billing period in months
    pub period_unit: Option<String>, // "month" or "year"
    pub trial_period: Option<i32>, // trial period in days
    pub trial_period_unit: Option<String>, // "day" or "month"
    pub pricing_model: String, // "flat_fee", "per_unit", "tiered", "volume", "stairstep"
    pub free_quantity: Option<i32>,
    pub status: Option<String>, // "active" or "archived"
    pub currency_code: String,  // "USD", "EUR", etc.
    pub metadata: Option<JsonValue>,
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
