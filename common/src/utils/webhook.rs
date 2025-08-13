use hmac::{Hmac, Mac};
use sha2::Sha256;
use serde_json::Value;
use std::net::IpAddr;
use ipnetwork::IpNetwork;

type HmacSha256 = Hmac<Sha256>;

/// Generate HMAC signature for webhook payload
pub fn generate_hmac_signature(secret: &str, payload: &Value) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC can take key of any size");
    
    let payload_str = serde_json::to_string(payload).unwrap_or_default();
    mac.update(payload_str.as_bytes());
    
    let result = mac.finalize();
    format!("sha256={}", hex::encode(result.into_bytes()))
}

/// Verify webhook signature
pub fn verify_webhook_signature(secret: &str, payload: &str, signature: &str) -> bool {
    let expected = {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .expect("HMAC can take key of any size");
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

/// Check if an IP address is in the allowlist
/// Supports individual IPs and CIDR ranges
pub fn is_ip_allowed(ip: &IpAddr, allowlist: &[String]) -> bool {
    for allowed in allowlist {
        // Check if it's a CIDR range
        if allowed.contains('/') {
            if let Ok(network) = allowed.parse::<IpNetwork>() {
                if network.contains(*ip) {
                    return true;
                }
            }
        } else {
            // Check if it's an individual IP
            if let Ok(allowed_ip) = allowed.parse::<IpAddr>() {
                if &allowed_ip == ip {
                    return true;
                }
            }
        }
    }
    false
}

/// Resolve a URL to its IP addresses using Google DNS-over-HTTPS (parallel queries)
pub async fn resolve_url_to_ips(url: &str) -> Result<Vec<IpAddr>, String> {
    // Parse the URL to get the host
    let parsed = url::Url::parse(url)
        .map_err(|e| format!("Invalid URL: {}", e))?;
    
    let host = parsed.host_str()
        .ok_or_else(|| "URL has no host".to_string())?;
    
    // If the host is already an IP, return it
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }
    
    // Use Google DNS-over-HTTPS API with parallel queries
    let client = reqwest::Client::new();
    
    // Create futures for both A and AAAA record queries
    let host_a = host.to_string();
    let host_aaaa = host.to_string();
    let client_a = client.clone();
    let client_aaaa = client;
    
    let a_query = async move {
        let response = client_a
            .get("https://dns.google/resolve")
            .query(&[("name", &host_a), ("type", &"A".to_string())])
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;
        
        match response {
            Ok(resp) if resp.status().is_success() => {
                resp.json::<serde_json::Value>().await.ok()
            },
            _ => None
        }
    };
    
    let aaaa_query = async move {
        let response = client_aaaa
            .get("https://dns.google/resolve")
            .query(&[("name", &host_aaaa), ("type", &"AAAA".to_string())])
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;
        
        match response {
            Ok(resp) if resp.status().is_success() => {
                resp.json::<serde_json::Value>().await.ok()
            },
            _ => None
        }
    };
    
    // Execute both queries in parallel
    let (a_result, aaaa_result) = futures::join!(a_query, aaaa_query);
    
    let mut ips = Vec::new();
    
    // Process A record results
    if let Some(data) = a_result {
        if let Some(answers) = data["Answer"].as_array() {
            for answer in answers {
                if let Some(ip_str) = answer["data"].as_str() {
                    if let Ok(ip) = ip_str.parse::<IpAddr>() {
                        ips.push(ip);
                    }
                }
            }
        }
    }
    
    // Process AAAA record results
    if let Some(data) = aaaa_result {
        if let Some(answers) = data["Answer"].as_array() {
            for answer in answers {
                if let Some(ip_str) = answer["data"].as_str() {
                    if let Ok(ip) = ip_str.parse::<IpAddr>() {
                        ips.push(ip);
                    }
                }
            }
        }
    }
    
    if ips.is_empty() {
        Err(format!("No IP addresses found for host: {}", host))
    } else {
        Ok(ips)
    }
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
        assert!(!verify_webhook_signature(secret, &payload_str, "invalid_signature"));
    }
}