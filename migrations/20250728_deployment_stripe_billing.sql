-- Create tables for deployment-level Stripe billing integration

-- Deployment Stripe accounts (Connect integration)
CREATE TABLE deployment_stripe_accounts (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deployment_id BIGINT NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    stripe_account_id TEXT NOT NULL UNIQUE,
    stripe_user_id TEXT,
    access_token_encrypted TEXT,
    refresh_token_encrypted TEXT,
    account_type TEXT NOT NULL CHECK (account_type IN ('standard', 'express', 'custom')),
    charges_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    details_submitted BOOLEAN NOT NULL DEFAULT FALSE,
    setup_completed_at TIMESTAMPTZ,
    onboarding_url TEXT,
    dashboard_url TEXT,
    country TEXT,
    default_currency TEXT,
    metadata JSONB DEFAULT '{}',
    
    CONSTRAINT unique_deployment_stripe_account UNIQUE (deployment_id)
);

-- Billing plans available for deployments
CREATE TABLE deployment_billing_plans (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deployment_id BIGINT NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    stripe_price_id TEXT NOT NULL,
    billing_interval TEXT NOT NULL CHECK (billing_interval IN ('month', 'year', 'week', 'day')),
    amount_cents BIGINT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'usd',
    trial_period_days INTEGER,
    usage_type TEXT CHECK (usage_type IN ('licensed', 'metered')),
    features JSONB DEFAULT '{}',
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    display_order INTEGER DEFAULT 0,
    
    CONSTRAINT unique_deployment_plan_name UNIQUE (deployment_id, name),
    CONSTRAINT unique_deployment_stripe_price UNIQUE (deployment_id, stripe_price_id)
);

-- Customer subscriptions per deployment
CREATE TABLE deployment_subscriptions (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deployment_id BIGINT NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    user_id BIGINT REFERENCES users(id) ON DELETE SET NULL,
    stripe_subscription_id TEXT NOT NULL UNIQUE,
    stripe_customer_id TEXT NOT NULL,
    billing_plan_id BIGINT REFERENCES deployment_billing_plans(id) ON DELETE SET NULL,
    status TEXT NOT NULL CHECK (status IN ('incomplete', 'incomplete_expired', 'trialing', 'active', 'past_due', 'canceled', 'unpaid', 'paused')),
    current_period_start TIMESTAMPTZ NOT NULL,
    current_period_end TIMESTAMPTZ NOT NULL,
    trial_start TIMESTAMPTZ,
    trial_end TIMESTAMPTZ,
    cancel_at_period_end BOOLEAN NOT NULL DEFAULT FALSE,
    canceled_at TIMESTAMPTZ,
    ended_at TIMESTAMPTZ,
    collection_method TEXT DEFAULT 'charge_automatically' CHECK (collection_method IN ('charge_automatically', 'send_invoice')),
    customer_email TEXT,
    customer_name TEXT,
    metadata JSONB DEFAULT '{}'
);

-- Invoice tracking
CREATE TABLE deployment_invoices (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deployment_id BIGINT NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    subscription_id BIGINT REFERENCES deployment_subscriptions(id) ON DELETE SET NULL,
    stripe_invoice_id TEXT NOT NULL UNIQUE,
    stripe_customer_id TEXT NOT NULL,
    amount_due_cents BIGINT NOT NULL,
    amount_paid_cents BIGINT DEFAULT 0,
    currency TEXT NOT NULL DEFAULT 'usd',
    status TEXT NOT NULL CHECK (status IN ('draft', 'open', 'paid', 'uncollectible', 'void')),
    invoice_pdf_url TEXT,
    hosted_invoice_url TEXT,
    invoice_number TEXT,
    due_date TIMESTAMPTZ,
    paid_at TIMESTAMPTZ,
    period_start TIMESTAMPTZ,
    period_end TIMESTAMPTZ,
    attempt_count INTEGER DEFAULT 0,
    next_payment_attempt TIMESTAMPTZ,
    metadata JSONB DEFAULT '{}'
);

-- Usage tracking for metered billing
CREATE TABLE deployment_usage_records (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deployment_id BIGINT NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    subscription_id BIGINT REFERENCES deployment_subscriptions(id) ON DELETE CASCADE,
    metric_name TEXT NOT NULL,
    quantity BIGINT NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    stripe_usage_record_id TEXT,
    billing_period_start TIMESTAMPTZ NOT NULL,
    billing_period_end TIMESTAMPTZ NOT NULL,
    metadata JSONB DEFAULT '{}'
);

-- Payment methods for customers
CREATE TABLE deployment_payment_methods (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deployment_id BIGINT NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    user_id BIGINT REFERENCES users(id) ON DELETE CASCADE,
    stripe_payment_method_id TEXT NOT NULL UNIQUE,
    stripe_customer_id TEXT NOT NULL,
    type TEXT NOT NULL CHECK (type IN ('card', 'bank_account')),
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    card_brand TEXT,
    card_last4 TEXT,
    card_exp_month INTEGER,
    card_exp_year INTEGER,
    bank_name TEXT,
    bank_last4 TEXT,
    metadata JSONB DEFAULT '{}'
);

-- Billing events audit log
CREATE TABLE deployment_billing_events (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deployment_id BIGINT NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    user_id BIGINT REFERENCES users(id) ON DELETE SET NULL,
    event_type TEXT NOT NULL,
    stripe_event_id TEXT,
    resource_type TEXT NOT NULL,
    resource_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'processed', 'failed')),
    error_message TEXT,
    event_data JSONB DEFAULT '{}',
    processed_at TIMESTAMPTZ
);

-- Indexes for performance
CREATE INDEX idx_deployment_stripe_accounts_deployment_id ON deployment_stripe_accounts(deployment_id);
CREATE INDEX idx_deployment_billing_plans_deployment_id ON deployment_billing_plans(deployment_id);
CREATE INDEX idx_deployment_billing_plans_active ON deployment_billing_plans(deployment_id, is_active) WHERE is_active = true;
CREATE INDEX idx_deployment_subscriptions_deployment_id ON deployment_subscriptions(deployment_id);
CREATE INDEX idx_deployment_subscriptions_user_id ON deployment_subscriptions(user_id);
CREATE INDEX idx_deployment_subscriptions_status ON deployment_subscriptions(deployment_id, status);
CREATE INDEX idx_deployment_subscriptions_stripe_id ON deployment_subscriptions(stripe_subscription_id);
CREATE INDEX idx_deployment_invoices_deployment_id ON deployment_invoices(deployment_id);
CREATE INDEX idx_deployment_invoices_subscription_id ON deployment_invoices(subscription_id);
CREATE INDEX idx_deployment_invoices_stripe_id ON deployment_invoices(stripe_invoice_id);
CREATE INDEX idx_deployment_invoices_status ON deployment_invoices(deployment_id, status);
CREATE INDEX idx_deployment_usage_records_deployment_id ON deployment_usage_records(deployment_id);
CREATE INDEX idx_deployment_usage_records_subscription_id ON deployment_usage_records(subscription_id);
CREATE INDEX idx_deployment_usage_records_metric_timestamp ON deployment_usage_records(deployment_id, metric_name, timestamp);
CREATE INDEX idx_deployment_payment_methods_deployment_id ON deployment_payment_methods(deployment_id);
CREATE INDEX idx_deployment_payment_methods_user_id ON deployment_payment_methods(user_id);
CREATE INDEX idx_deployment_payment_methods_customer_id ON deployment_payment_methods(stripe_customer_id);
CREATE INDEX idx_deployment_billing_events_deployment_id ON deployment_billing_events(deployment_id);
CREATE INDEX idx_deployment_billing_events_type_status ON deployment_billing_events(event_type, status);
CREATE INDEX idx_deployment_billing_events_stripe_id ON deployment_billing_events(stripe_event_id);