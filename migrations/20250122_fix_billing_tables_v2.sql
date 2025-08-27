-- Drop existing subscriptions table
DROP TABLE IF EXISTS subscriptions CASCADE;

-- Create subscriptions table that supports both user and organization billing
CREATE TABLE subscriptions (
    id BIGINT PRIMARY KEY,
    user_id BIGINT REFERENCES users(id) ON DELETE CASCADE,
    organization_id BIGINT REFERENCES organizations(id) ON DELETE CASCADE,
    chargebee_customer_id VARCHAR(255) NOT NULL,
    chargebee_subscription_id VARCHAR(255) NOT NULL UNIQUE,
    status VARCHAR(50) NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    -- Ensure subscription is tied to either a user OR an organization, not both
    CONSTRAINT subscription_owner CHECK (
        (user_id IS NOT NULL AND organization_id IS NULL) OR 
        (user_id IS NULL AND organization_id IS NOT NULL)
    ),
    -- Ensure only one subscription per user
    CONSTRAINT unique_user_subscription UNIQUE(user_id),
    -- Ensure only one subscription per organization
    CONSTRAINT unique_org_subscription UNIQUE(organization_id)
);

CREATE INDEX idx_subscriptions_user_id ON subscriptions(user_id) WHERE user_id IS NOT NULL;
CREATE INDEX idx_subscriptions_organization_id ON subscriptions(organization_id) WHERE organization_id IS NOT NULL;
CREATE INDEX idx_subscriptions_chargebee_subscription_id ON subscriptions(chargebee_subscription_id);