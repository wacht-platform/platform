-- Simple subscription table
CREATE TABLE subscriptions (
    id BIGINT PRIMARY KEY,
    project_id BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    chargebee_customer_id VARCHAR(255) NOT NULL,
    chargebee_subscription_id VARCHAR(255) NOT NULL UNIQUE,
    status VARCHAR(50) NOT NULL, -- 'active', 'cancelled', 'past_due', 'trialing'
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(project_id)
);

CREATE INDEX idx_subscriptions_project_id ON subscriptions (project_id);
CREATE INDEX idx_subscriptions_chargebee_subscription_id ON subscriptions (chargebee_subscription_id);