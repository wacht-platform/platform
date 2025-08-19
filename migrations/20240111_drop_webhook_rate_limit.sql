-- Drop rate_limit_per_minute column from webhook_apps table
ALTER TABLE webhook_apps DROP COLUMN IF EXISTS rate_limit_per_minute;