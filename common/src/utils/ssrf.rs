use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use url::Url;

/// Check if an IP address is private or reserved (potential SSRF target)
pub fn is_private_or_reserved_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_private_ipv4(&ip),
        IpAddr::V6(ip) => is_private_ipv6(&ip),
    }
}

fn is_private_ipv4(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();

    // Private ranges
    ip.is_private() ||

    // Loopback (127.0.0.0/8)
    ip.is_loopback() ||

    // Link-local (169.254.0.0/16)
    ip.is_link_local() ||

    // Broadcast
    ip.is_broadcast() ||

    // Documentation (192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24)
    ip.is_documentation() ||

    // AWS/Cloud metadata endpoint (169.254.169.254)
    octets[0] == 169 && octets[1] == 254 && octets[2] == 169 && octets[3] == 254 ||

    // Unspecified
    ip.is_unspecified() ||

    // Multicast
    octets[0] >= 224 && octets[0] <= 239
}

fn is_private_ipv6(ip: &Ipv6Addr) -> bool {
    // Loopback (::1)
    ip.is_loopback() ||

    // Unspecified (::)
    ip.is_unspecified() ||

    // Multicast
    ip.is_multicast() ||

    // Link-local (fe80::/10)
    (ip.segments()[0] & 0xffc0) == 0xfe80 ||

    // Unique local address (fc00::/7)
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

/// Validate webhook URL to prevent SSRF attacks
pub fn validate_webhook_url(url: &str) -> Result<(), String> {
    // Parse URL
    let parsed_url = Url::parse(url).map_err(|_| "Invalid URL format".to_string())?;

    // Check scheme (only allow http/https)
    let scheme = parsed_url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!(
            "Invalid URL scheme: {}. Only http and https are allowed",
            scheme
        ));
    }

    // Get host
    let host = parsed_url
        .host_str()
        .ok_or_else(|| "URL must have a host".to_string())?;

    // Don't allow URLs with user info (http://user:pass@host)
    if parsed_url.username() != "" || parsed_url.password().is_some() {
        return Err("URLs with credentials are not allowed".to_string());
    }

    // Check for localhost variations
    if host == "localhost"
        || host.ends_with(".localhost")
        || host == "0.0.0.0"
        || host == "[::]"
        || host == "::1"
    {
        return Err(format!("Localhost URLs are not allowed: {}", host));
    }

    // Try to resolve hostname to IP and check if it's private
    // Note: This is a basic check. In production, you should resolve this
    // at request time (not just at endpoint creation) to prevent DNS rebinding attacks
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_or_reserved_ip(ip) {
            return Err(format!(
                "Private or reserved IP addresses are not allowed: {}",
                ip
            ));
        }
    }

    // Check for common dangerous hostnames
    let dangerous_hosts = [
        "metadata.google.internal",
        "169.254.169.254",
        "metadata",
        "instance-data",
    ];

    for dangerous in &dangerous_hosts {
        if host.contains(dangerous) {
            return Err(format!("Potentially dangerous hostname detected: {}", host));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_private_ipv4() {
        assert!(is_private_or_reserved_ip("10.0.0.1".parse().unwrap()));
        assert!(is_private_or_reserved_ip("172.16.0.1".parse().unwrap()));
        assert!(is_private_or_reserved_ip("192.168.1.1".parse().unwrap()));
        assert!(is_private_or_reserved_ip("127.0.0.1".parse().unwrap()));
        assert!(is_private_or_reserved_ip(
            "169.254.169.254".parse().unwrap()
        ));

        assert!(!is_private_or_reserved_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_private_or_reserved_ip("1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn test_validate_webhook_url() {
        // Valid URLs
        assert!(validate_webhook_url("https://example.com/webhook").is_ok());
        assert!(validate_webhook_url("http://api.example.com/hooks").is_ok());

        // Invalid - private IPs
        assert!(validate_webhook_url("http://10.0.0.1/webhook").is_err());
        assert!(validate_webhook_url("http://192.168.1.1/webhook").is_err());
        assert!(validate_webhook_url("http://127.0.0.1/webhook").is_err());

        // Invalid - localhost
        assert!(validate_webhook_url("http://localhost/webhook").is_err());

        // Invalid - metadata endpoint
        assert!(validate_webhook_url("http://169.254.169.254/latest/meta-data").is_err());

        // Invalid - credentials
        assert!(validate_webhook_url("http://user:pass@example.com/webhook").is_err());

        // Invalid - scheme
        assert!(validate_webhook_url("ftp://example.com/webhook").is_err());
        assert!(validate_webhook_url("file:///etc/passwd").is_err());
    }
}
