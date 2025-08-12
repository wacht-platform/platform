-- Add unique constraint on webhook app name per deployment
-- This ensures each deployment can have multiple apps but with unique names
ALTER TABLE webhook_apps 
ADD CONSTRAINT unique_webhook_app_name_per_deployment 
UNIQUE (deployment_id, name);

-- Add index for faster lookups by deployment and name
CREATE INDEX IF NOT EXISTS idx_webhook_apps_deployment_name 
ON webhook_apps(deployment_id, name);