-- Webhook Apps (like GitHub Apps, Stripe Apps)
CREATE TABLE webhook_apps (
    id BIGSERIAL PRIMARY KEY,
    deployment_id BIGINT NOT NULL,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    signing_secret VARCHAR(255) NOT NULL,
    is_active BOOLEAN DEFAULT true,
    rate_limit_per_minute INT DEFAULT 60,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(deployment_id, name)
);

-- Available events per app
CREATE TABLE webhook_app_events (
    id BIGSERIAL PRIMARY KEY,
    app_id BIGINT REFERENCES webhook_apps(id) ON DELETE CASCADE,
    event_name VARCHAR(100) NOT NULL,
    description TEXT,
    schema JSONB,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(app_id, event_name)
);

-- Webhook endpoints (subscribers)
CREATE TABLE webhook_endpoints (
    id BIGSERIAL PRIMARY KEY,
    app_id BIGINT REFERENCES webhook_apps(id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    description TEXT,
    headers JSONB DEFAULT '{}',
    is_active BOOLEAN DEFAULT true,
    max_retries INT DEFAULT 5,
    timeout_seconds INT DEFAULT 30,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Event subscriptions (many-to-many)
CREATE TABLE webhook_endpoint_subscriptions (
    endpoint_id BIGINT REFERENCES webhook_endpoints(id) ON DELETE CASCADE,
    event_id BIGINT REFERENCES webhook_app_events(id) ON DELETE CASCADE,
    filter_rules JSONB,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    PRIMARY KEY (endpoint_id, event_id)
);

-- Active delivery queue
CREATE TABLE active_webhook_deliveries (
    id BIGSERIAL PRIMARY KEY,
    endpoint_id BIGINT REFERENCES webhook_endpoints(id) ON DELETE CASCADE,
    event_name VARCHAR(100) NOT NULL,
    payload_s3_key VARCHAR(255) NOT NULL,
    payload_size_bytes INT NOT NULL,
    signature VARCHAR(255),
    attempts INT DEFAULT 0,
    max_attempts INT DEFAULT 5,
    next_retry_at TIMESTAMPTZ DEFAULT NOW(),
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Indexes for performance
CREATE INDEX idx_webhook_apps_deployment ON webhook_apps(deployment_id) WHERE is_active = true;
CREATE INDEX idx_webhook_endpoints_app ON webhook_endpoints(app_id) WHERE is_active = true;
CREATE INDEX idx_active_deliveries_retry ON active_webhook_deliveries(next_retry_at);
CREATE INDEX idx_active_deliveries_endpoint ON active_webhook_deliveries(endpoint_id);
CREATE INDEX idx_webhook_subscriptions_event ON webhook_endpoint_subscriptions(event_id);

-- Function to update updated_at timestamp
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Add triggers for updated_at
CREATE TRIGGER update_webhook_apps_updated_at BEFORE UPDATE ON webhook_apps
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_webhook_endpoints_updated_at BEFORE UPDATE ON webhook_endpoints
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();