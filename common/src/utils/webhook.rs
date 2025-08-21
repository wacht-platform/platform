use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Generate HMAC signature for webhook payload
pub fn generate_hmac_signature(secret: &str, payload: &Value) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");

    let payload_str = serde_json::to_string(payload).unwrap_or_default();
    mac.update(payload_str.as_bytes());

    let result = mac.finalize();
    format!("sha256={}", hex::encode(result.into_bytes()))
}

/// Verify webhook signature
pub fn verify_webhook_signature(secret: &str, payload: &str, signature: &str) -> bool {
    let expected = {
        let mut mac =
            HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
        mac.update(payload.as_bytes());
        let result = mac.finalize();
        format!("sha256={}", hex::encode(result.into_bytes()))
    };

    // Constant time comparison to prevent timing attacks
    constant_time_eq(expected.as_bytes(), signature.as_bytes())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hmac_signature() {
        let secret = "whsec_test_secret";
        let payload = serde_json::json!({
            "event": "user.created",
            "user_id": "123"
        });

        let signature = generate_hmac_signature(secret, &payload);
        assert!(signature.starts_with("sha256="));

        let payload_str = serde_json::to_string(&payload).unwrap();
        assert!(verify_webhook_signature(secret, &payload_str, &signature));
        assert!(!verify_webhook_signature(
            secret,
            &payload_str,
            "invalid_signature"
        ));
    }
}
