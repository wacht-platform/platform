-- Add app_name column to webhook_endpoints table for better filtering
ALTER TABLE webhook_endpoints 
ADD COLUMN IF NOT EXISTS app_name VARCHAR(255);

-- Update existing records to populate app_name from webhook_apps
UPDATE webhook_endpoints e
SET app_name = a.name
FROM webhook_apps a
WHERE e.app_id = a.id
AND e.app_name IS NULL;

-- Make app_name NOT NULL after populating existing records
ALTER TABLE webhook_endpoints 
ALTER COLUMN app_name SET NOT NULL;

-- Add index on app_name for better query performance
CREATE INDEX IF NOT EXISTS idx_webhook_endpoints_app_name 
ON webhook_endpoints(app_name);

-- Add composite index for deployment_id and app_name filtering
CREATE INDEX IF NOT EXISTS idx_webhook_endpoints_deployment_app 
ON webhook_endpoints(deployment_id, app_name);

-- Add app_name to webhook_subscriptions for consistency
ALTER TABLE webhook_subscriptions 
ADD COLUMN IF NOT EXISTS app_name VARCHAR(255);

-- Update existing webhook_subscriptions records
UPDATE webhook_subscriptions s
SET app_name = a.name
FROM webhook_endpoints e
JOIN webhook_apps a ON e.app_id = a.id
WHERE s.endpoint_id = e.id
AND s.app_name IS NULL;

-- Make app_name NOT NULL in webhook_subscriptions
ALTER TABLE webhook_subscriptions 
ALTER COLUMN app_name SET NOT NULL;

-- Add deployment_id to webhook_endpoints if not exists
ALTER TABLE webhook_endpoints 
ADD COLUMN IF NOT EXISTS deployment_id BIGINT;

-- Update deployment_id from webhook_apps
UPDATE webhook_endpoints e
SET deployment_id = a.deployment_id
FROM webhook_apps a
WHERE e.app_id = a.id
AND e.deployment_id IS NULL;

-- Make deployment_id NOT NULL
ALTER TABLE webhook_endpoints 
ALTER COLUMN deployment_id SET NOT NULL;

-- Add index on deployment_id
CREATE INDEX IF NOT EXISTS idx_webhook_endpoints_deployment_id 
ON webhook_endpoints(deployment_id);