-- Drop all existing webhook tables to rebuild with composite keys
DROP TABLE IF EXISTS active_webhook_deliveries CASCADE;
DROP TABLE IF EXISTS webhook_endpoint_subscriptions CASCADE;
DROP TABLE IF EXISTS webhook_endpoints CASCADE;
DROP TABLE IF EXISTS webhook_app_events CASCADE;
DROP TABLE IF EXISTS webhook_apps CASCADE;

-- Recreate webhook_apps with composite primary key
CREATE TABLE webhook_apps (
    deployment_id BIGINT NOT NULL,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    signing_secret VARCHAR(255) NOT NULL,
    is_active BOOLEAN DEFAULT true,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    PRIMARY KEY (deployment_id, name)
);

-- Available events per app with composite foreign key
CREATE TABLE webhook_app_events (
    deployment_id BIGINT NOT NULL,
    app_name VARCHAR(255) NOT NULL,
    event_name VARCHAR(100) NOT NULL,
    description TEXT,
    schema JSONB,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    PRIMARY KEY (deployment_id, app_name, event_name),
    FOREIGN KEY (deployment_id, app_name) REFERENCES webhook_apps(deployment_id, name) ON DELETE CASCADE
);

-- Webhook endpoints with composite foreign key
CREATE TABLE webhook_endpoints (
    id BIGSERIAL PRIMARY KEY,
    deployment_id BIGINT NOT NULL,
    app_name VARCHAR(255) NOT NULL,
    url TEXT NOT NULL,
    description TEXT,
    headers JSONB DEFAULT '{}',
    is_active BOOLEAN DEFAULT true,
    signing_secret VARCHAR(255),
    max_retries INT DEFAULT 5,
    timeout_seconds INT DEFAULT 30,
    failure_count INT DEFAULT 0,
    last_failure_at TIMESTAMPTZ,
    auto_disabled BOOLEAN DEFAULT false,
    auto_disabled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    FOREIGN KEY (deployment_id, app_name) REFERENCES webhook_apps(deployment_id, name) ON DELETE CASCADE
);

-- Event subscriptions with composite foreign key
CREATE TABLE webhook_endpoint_subscriptions (
    endpoint_id BIGINT REFERENCES webhook_endpoints(id) ON DELETE CASCADE,
    deployment_id BIGINT NOT NULL,
    app_name VARCHAR(255) NOT NULL,
    event_name VARCHAR(100) NOT NULL,
    filter_rules JSONB,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    PRIMARY KEY (endpoint_id, deployment_id, app_name, event_name),
    FOREIGN KEY (deployment_id, app_name, event_name) REFERENCES webhook_app_events(deployment_id, app_name, event_name) ON DELETE CASCADE
);

-- Active delivery queue
CREATE TABLE active_webhook_deliveries (
    id BIGSERIAL PRIMARY KEY,
    endpoint_id BIGINT REFERENCES webhook_endpoints(id) ON DELETE CASCADE,
    deployment_id BIGINT NOT NULL,
    app_name VARCHAR(255) NOT NULL,
    event_name VARCHAR(100) NOT NULL,
    payload_s3_key VARCHAR(255) NOT NULL,
    payload_size_bytes INT NOT NULL,
    signature VARCHAR(255),
    attempts INT DEFAULT 0,
    max_attempts INT DEFAULT 5,
    next_retry_at TIMESTAMPTZ DEFAULT NOW(),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    FOREIGN KEY (deployment_id, app_name) REFERENCES webhook_apps(deployment_id, name) ON DELETE CASCADE
);

-- Indexes for performance
CREATE INDEX idx_webhook_apps_deployment ON webhook_apps(deployment_id) WHERE is_active = true;
CREATE INDEX idx_webhook_endpoints_app ON webhook_endpoints(deployment_id, app_name) WHERE is_active = true;
CREATE INDEX idx_webhook_endpoints_failure ON webhook_endpoints(deployment_id, failure_count) WHERE is_active = true;
CREATE INDEX idx_active_deliveries_retry ON active_webhook_deliveries(next_retry_at);
CREATE INDEX idx_active_deliveries_endpoint ON active_webhook_deliveries(endpoint_id);
CREATE INDEX idx_webhook_subscriptions_event ON webhook_endpoint_subscriptions(deployment_id, app_name, event_name);

-- Recreate the update trigger
CREATE TRIGGER update_webhook_apps_updated_at BEFORE UPDATE ON webhook_apps
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_webhook_endpoints_updated_at BEFORE UPDATE ON webhook_endpoints
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();