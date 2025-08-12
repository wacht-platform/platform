-- Add IP allowlist support to webhook endpoints
ALTER TABLE webhook_endpoints 
ADD COLUMN ip_allowlist JSONB DEFAULT NULL;

-- Add comment for documentation
COMMENT ON COLUMN webhook_endpoints.ip_allowlist IS 'Optional array of allowed IP addresses or CIDR ranges. When set, deliveries will only be sent if the resolved IP matches.';

-- Example: ["192.168.1.100", "10.0.0.0/8", "2001:db8::/32"]