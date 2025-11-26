use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Extract raw secret for HMAC (handles whsec_ prefix)
fn extract_secret_for_hmac(secret: &str) -> Vec<u8> {
    let webhook_secret = if secret.starts_with("whsec_") {
        &secret[6..]
    } else {
        secret
    };

    base64::engine::general_purpose::STANDARD
        .decode(&webhook_secret)
        .unwrap()
}

/// Generate HMAC signature following Standard Webhooks specification
/// Format: webhook-id.timestamp.payload
/// Returns: v1,<base64_signature>
pub fn generate_webhook_signature(
    secret: &str,
    webhook_id: &str,
    timestamp: i64,
    payload: &Value,
) -> String {
    let raw_secret = extract_secret_for_hmac(secret);

    let mut mac = HmacSha256::new_from_slice(&raw_secret).expect("HMAC can take key of any size");

    let payload_str = serde_json::to_string(payload).unwrap_or_default();
    let signed_content = format!("{}.{}.{}", webhook_id, timestamp, payload_str);
    mac.update(signed_content.as_bytes());

    let result = mac.finalize();
    let base64_sig = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        result.into_bytes(),
    );
    format!("v1,{}", base64_sig)
}

/// Verify webhook signature following Standard Webhooks specification
/// signature format: "v1,<base64_signature>"
pub fn verify_webhook_signature(
    secret: &str,
    webhook_id: &str,
    timestamp: i64,
    payload: &str,
    signature: &str,
) -> bool {
    // Extract signature after "v1," prefix
    let actual_signature = if let Some(sig) = signature.strip_prefix("v1,") {
        sig
    } else if let Some(sig) = signature.strip_prefix("v1a,") {
        sig
    } else {
        signature
    };

    let raw_secret = extract_secret_for_hmac(secret);

    let expected = {
        let mut mac =
            HmacSha256::new_from_slice(&raw_secret).expect("HMAC can take key of any size");
        let signed_content = format!("{}.{}.{}", webhook_id, timestamp, payload);
        mac.update(signed_content.as_bytes());
        let result = mac.finalize();
        base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            result.into_bytes(),
        )
    };

    // Constant time comparison to prevent timing attacks
    constant_time_eq(expected.as_bytes(), actual_signature.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut result = 0u8;
    for (a_byte, b_byte) in a.iter().zip(b.iter()) {
        result |= a_byte ^ b_byte;
    }

    result == 0
}
