-- API Key Applications (similar to webhook_apps)
CREATE TABLE api_key_apps (
    id BIGSERIAL PRIMARY KEY,
    deployment_id BIGINT NOT NULL REFERENCES deployments(id),
    name VARCHAR(255) NOT NULL,
    description TEXT,
    is_active BOOLEAN DEFAULT true,
    rate_limit_per_minute INT DEFAULT 60,
    rate_limit_per_hour INT DEFAULT 1000,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    deleted_at TIMESTAMPTZ,
    UNIQUE(deployment_id, name)
);

-- API Keys
CREATE TABLE api_keys (
    id BIGSERIAL PRIMARY KEY,
    app_id BIGINT NOT NULL REFERENCES api_key_apps(id) ON DELETE CASCADE,
    deployment_id BIGINT NOT NULL REFERENCES deployments(id),
    name VARCHAR(255) NOT NULL,
    key_prefix VARCHAR(10) NOT NULL, -- 'sk_live_', 'sk_test_', 'pk_live_', 'pk_test_'
    key_hash VARCHAR(255) NOT NULL UNIQUE, -- SHA-256 hash of the full key
    key_suffix VARCHAR(8) NOT NULL, -- last 8 chars for identification
    permissions JSONB DEFAULT '[]', -- array of permission strings
    metadata JSONB DEFAULT '{}', -- custom metadata
    expires_at TIMESTAMPTZ, -- optional expiration
    last_used_at TIMESTAMPTZ,
    is_active BOOLEAN DEFAULT true,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    revoked_at TIMESTAMPTZ,
    revoked_reason TEXT
);

-- Create indexes for api_keys table
CREATE INDEX idx_api_keys_key_prefix ON api_keys (key_prefix);
CREATE INDEX idx_api_keys_app_id ON api_keys (app_id);
CREATE INDEX idx_api_keys_expires_at ON api_keys (expires_at);
CREATE INDEX idx_api_keys_key_hash ON api_keys (key_hash);

-- Optional: Granular permission scopes
CREATE TABLE api_key_scopes (
    id BIGSERIAL PRIMARY KEY,
    api_key_id BIGINT NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    resource_type VARCHAR(100) NOT NULL, -- 'users', 'organizations', 'workspaces'
    resource_id VARCHAR(255), -- optional specific resource ID
    actions JSONB NOT NULL, -- ['read', 'write', 'delete']
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Create index for api_key_scopes table
CREATE INDEX idx_api_key_scopes_key_id ON api_key_scopes (api_key_id);