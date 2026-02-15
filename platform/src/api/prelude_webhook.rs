use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{Datelike, Utc};
use serde::Deserialize;
use tracing::{error, info, warn};

use common::state::AppState;

#[derive(Debug, Deserialize)]
pub struct PreludeWebhookEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: PreludeEventPayload,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct PreludeEventPayload {
    pub metadata: serde_json::Value,
    pub price: PreludePrice,
    pub target: PreludeTarget,
    pub verification_id: String,
    pub time: String,
}

#[derive(Debug, Deserialize)]
pub struct PreludePrice {
    pub amount: f64,
    pub currency: String,
}

#[derive(Debug, Deserialize)]
pub struct PreludeTarget {
    #[serde(rename = "type")]
    pub target_type: String,
    pub value: String,
}

pub async fn handle_prelude_webhook(
    State(app_state): State<AppState>,
    Path(deployment_id): Path<i64>,
    headers: HeaderMap,
    body: String,
) -> Result<StatusCode, (StatusCode, String)> {
    if let Err(e) = verify_signature(&headers, &body) {
        warn!("[PRELUDE WEBHOOK] Signature verification failed: {}", e);
        return Err((StatusCode::UNAUTHORIZED, "Invalid signature".to_string()));
    }

    let event: PreludeWebhookEvent = serde_json::from_str(&body).map_err(|e| {
        error!("[PRELUDE WEBHOOK] Failed to parse payload: {}", e);
        (StatusCode::BAD_REQUEST, format!("Invalid payload: {}", e))
    })?;

    info!(
        "[PRELUDE WEBHOOK] Received event: {} (type: {})",
        event.id, event.event_type
    );

    if event.event_type != "verify.authentication" {
        info!(
            "[PRELUDE WEBHOOK] Ignoring non-authentication event: {}",
            event.event_type
        );
        return Ok(StatusCode::OK);
    }

    // Convert price to USD cents
    let cost_cents = convert_to_usd_cents(
        event.payload.price.amount,
        &event.payload.price.currency,
        &app_state,
    )
    .await
    .map_err(|e| {
        error!("[PRELUDE WEBHOOK] Currency conversion failed: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Currency conversion failed: {}", e),
        )
    })?;

    info!(
        "[PRELUDE WEBHOOK] Tracking SMS cost for deployment {}: {} USD cents (original: {}{})",
        deployment_id, cost_cents, event.payload.price.currency, event.payload.price.amount
    );

    if let Err(e) = track_sms_cost(deployment_id, cost_cents, &app_state).await {
        error!(
            "[PRELUDE WEBHOOK] Failed to track SMS cost for deployment {}: {}",
            deployment_id, e
        );
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to track cost".to_string(),
        ));
    }

    info!(
        "[PRELUDE WEBHOOK] Successfully tracked SMS cost for deployment {}",
        deployment_id
    );

    Ok(StatusCode::OK)
}

fn verify_signature(headers: &HeaderMap, body: &str) -> Result<(), String> {
    use base64::{Engine as _, engine::general_purpose};
    use rsa::{
        pkcs8::DecodePublicKey,
        pss::{Signature, VerifyingKey},
        signature::Verifier,
    };
    use sha2::Sha256;

    // Get signature from header
    let signature_header = headers
        .get("x-webhook-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| "Missing X-Webhook-Signature header".to_string())?;

    let signature_b64 = signature_header
        .strip_prefix("rsassa-pss-sha256=")
        .ok_or_else(|| "Invalid signature format, expected rsassa-pss-sha256=".to_string())?;

    let signature_bytes = general_purpose::STANDARD
        .decode(signature_b64)
        .map_err(|e| format!("Failed to decode signature: {}", e))?;

    let signature = Signature::try_from(signature_bytes.as_slice())
        .map_err(|e| format!("Invalid signature format: {}", e))?;

    let public_key_pem = std::env::var("PRELUDE_WEBHOOK_PUBLIC_KEY")
        .map_err(|_| "PRELUDE_WEBHOOK_PUBLIC_KEY not configured")?;

    let public_key = rsa::RsaPublicKey::from_public_key_pem(&public_key_pem)
        .map_err(|e| format!("Failed to parse public key: {}", e))?;

    let verifying_key = VerifyingKey::<Sha256>::new(public_key);

    // Verify signature
    verifying_key
        .verify(body.as_bytes(), &signature)
        .map_err(|e| format!("Signature verification failed: {}", e))?;

    info!("[PRELUDE WEBHOOK] Signature verified successfully");

    Ok(())
}

async fn track_sms_cost(
    deployment_id: i64,
    cost_cents: i64,
    app_state: &AppState,
) -> Result<(), String> {
    let mut redis = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| format!("Redis connection failed: {}", e))?;

    let now = Utc::now();
    let period = format!("{}-{:02}", now.year(), now.month());
    let prefix = format!("billing:{}:deployment:{}", period, deployment_id);

    let mut pipe = redis::pipe();
    pipe.atomic()
        .zincr(&format!("{}:metrics", prefix), "sms_cost_cents", cost_cents)
        .ignore()
        .expire(&format!("{}:metrics", prefix), 5184000)
        .ignore()
        .zincr(
            &format!("billing:{}:dirty_deployments", period),
            deployment_id,
            1,
        )
        .ignore()
        .expire(&format!("billing:{}:dirty_deployments", period), 5184000)
        .ignore();

    let _: Result<(), redis::RedisError> = pipe.query_async(&mut redis).await;

    Ok(())
}

async fn convert_to_usd_cents(
    amount: f64,
    currency: &str,
    app_state: &AppState,
) -> Result<i64, String> {
    if currency.eq_ignore_ascii_case("USD") {
        return Ok((amount * 100.0).round() as i64);
    }

    if currency.eq_ignore_ascii_case("EUR") {
        let rate = get_eur_to_usd_rate(app_state).await?;
        let usd_amount = amount * rate;
        return Ok((usd_amount * 100.0).round() as i64);
    }

    Err(format!("Unsupported currency: {}", currency))
}

async fn get_eur_to_usd_rate(app_state: &AppState) -> Result<f64, String> {
    let mut redis = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| format!("Redis connection failed: {}", e))?;

    let cache_key = "exchange_rate:eur_to_usd";

    let cached_rate: Option<String> = redis::cmd("GET")
        .arg(cache_key)
        .query_async(&mut redis)
        .await
        .map_err(|e| format!("Redis GET failed: {}", e))?;

    if let Some(rate_str) = cached_rate {
        if let Ok(rate) = rate_str.parse::<f64>() {
            info!("[PRELUDE WEBHOOK] Using cached EUR/USD rate: {}", rate);
            return Ok(rate);
        }
    }

    info!("[PRELUDE WEBHOOK] Fetching fresh EUR/USD exchange rate");
    let rate = fetch_eur_to_usd_rate().await?;

    let _: Result<(), redis::RedisError> = redis::cmd("SETEX")
        .arg(cache_key)
        .arg(86400)
        .arg(rate.to_string())
        .query_async(&mut redis)
        .await;

    info!("[PRELUDE WEBHOOK] Cached new EUR/USD rate: {}", rate);
    Ok(rate)
}

async fn fetch_eur_to_usd_rate() -> Result<f64, String> {
    let url = "https://open.er-api.com/v6/latest/EUR";

    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Exchange rate API request failed: {}", e))?;

    if !response.status().is_success() {
        warn!("[PRELUDE WEBHOOK] Exchange rate API failed, using fallback rate 1.17");
        return Ok(1.17);
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse exchange rate response: {}", e))?;

    let rate = json["rates"]["USD"]
        .as_f64()
        .ok_or_else(|| "USD rate not found in response".to_string())?;

    Ok(rate)
}
