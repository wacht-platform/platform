use anyhow::{Result, anyhow};
use chrono::{Datelike, Utc};
use reqwest;
use serde::{Deserialize, Serialize};
use tracing::error;

#[derive(Clone, Serialize, Deserialize)]
pub struct SMSTask {
    #[serde(rename = "type")]
    pub task_type: String,
    pub deployment_id: u64,
    pub phone_number: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SMSOTPTask {
    pub deployment_id: u64,
    pub phone_number: String,
    pub user_id: u64,
    pub country_code: String,
}

#[derive(Deserialize)]
struct MessageCentralSendResponse {
    #[serde(rename = "responseCode")]
    response_code: u32, // Number at top level
    message: String,
    data: Option<MessageCentralSendData>,
    #[serde(rename = "errorMessage")]
    error_message: Option<String>,
}

#[derive(Deserialize)]
struct MessageCentralSendData {
    #[serde(rename = "verificationId")]
    verification_id: String,
    #[serde(rename = "responseCode")]
    response_code: String, // String inside data
    #[serde(rename = "errorMessage")]
    error_message: Option<String>,
}

use common::state::AppState;

pub async fn send_otp_sms(
    deployment_id: u64,
    phone_number: &str,
    country_code: &str,
    app_state: &AppState,
) -> Result<String> {
    let customer_id = std::env::var("MESSAGE_CENTRAL_CUSTOMER_ID")
        .map_err(|_| anyhow!("MESSAGE_CENTRAL_CUSTOMER_ID not configured"))?;
    let auth_token = std::env::var("MESSAGE_CENTRAL_AUTH_TOKEN")
        .map_err(|_| anyhow!("MESSAGE_CENTRAL_AUTH_TOKEN not configured"))?;

    let clean_phone = phone_number.trim_start_matches('+');
    let clean_country_code = country_code.trim_start_matches('+');

    let url = format!(
        "https://cpaas.messagecentral.com/verification/v3/send?countryCode={}&customerId={}&flowType=SMS&mobileNumber={}&otpLength=6",
        clean_country_code, customer_id, clean_phone
    );

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("authToken", auth_token)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to send SMS: {}", e))?;

    let status = response.status();
    let response_text = response.text().await?;

    if !status.is_success() {
        error!("MessageCentral API error: {} - {}", status, response_text);
        return Err(anyhow!("SMS send failed with status {}", status));
    }

    let mc_response: MessageCentralSendResponse =
        serde_json::from_str(&response_text).map_err(|e| {
            anyhow!(
                "Failed to parse MessageCentral response: {} - Response: {}",
                e,
                response_text
            )
        })?;

    if mc_response.response_code != 200 {
        error!(
            "MessageCentral error: {} - {}",
            mc_response.response_code, mc_response.message
        );
        return Err(anyhow!(
            "SMS send failed: {}",
            mc_response.error_message.unwrap_or(mc_response.message)
        ));
    }

    if let Some(data) = mc_response.data {
        if data.response_code != "200" {
            return Err(anyhow!(
                "SMS send failed: {}",
                data.error_message
                    .unwrap_or_else(|| "Unknown error".to_string())
            ));
        }

        let cache_key = format!(
            "sms_verification:{}:{}:{}",
            deployment_id, country_code, phone_number
        );

        let mut conn = app_state
            .redis_client
            .get_connection()
            .map_err(|e| anyhow!("Failed to get Redis connection: {}", e))?;

        redis::cmd("SETEX")
            .arg(&cache_key)
            .arg(600) // 10 minutes expiry
            .arg(&data.verification_id)
            .query::<()>(&mut conn)
            .map_err(|e| anyhow!("Failed to store verification data: {}", e))?;

        track_sms_billing(deployment_id, &country_code, &phone_number, &app_state).await;

        Ok(format!(
            "SMS sent with verification ID: {}",
            data.verification_id
        ))
    } else {
        Err(anyhow!("No data in MessageCentral response"))
    }
}

async fn track_sms_billing(
    deployment_id: u64,
    country_code: &str,
    _phone_number: &str,
    app_state: &common::state::AppState,
) {
    use queries::{GetSmsPricingQuery, Query};

    let cost_cents: f64 = GetSmsPricingQuery::new(country_code.to_string())
        .execute(app_state)
        .await
        .ok()
        .flatten()
        .map(|d| d.to_string().parse::<f64>().unwrap_or(0.495))
        .unwrap_or(0.495);

    if let Ok(mut conn) = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
    {
        let now = Utc::now();
        let period = format!("{}-{:02}", now.year(), now.month());
        let prefix = format!("billing:{}:deployment:{}", period, deployment_id);

        let mut pipe = redis::pipe();
        pipe.atomic()
            .zincr(&format!("{}:metrics", prefix), "sms_count", 1)
            .ignore()
            .zincr(
                &format!("{}:metrics", prefix),
                "sms_cost_cents",
                cost_cents as i64,
            )
            .ignore()
            .expire(&format!("{}:metrics", prefix), 5184000)
            .ignore()
            .zincr(
                &format!("billing:{}:dirty_deployments", period),
                deployment_id as i64,
                1,
            )
            .ignore()
            .expire(&format!("billing:{}:dirty_deployments", period), 5184000)
            .ignore();

        let _: Result<(), redis::RedisError> = pipe.query_async(&mut conn).await;
    }
}
