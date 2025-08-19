-- Convert webhook and API key tables to use Snowflake IDs instead of sequential IDs
-- This migration removes SERIAL columns and makes them regular BIGINT columns
-- The application will generate Snowflake IDs before insertion

-- Drop the serial sequences and convert to regular BIGINT columns
ALTER TABLE webhook_apps ALTER COLUMN id DROP DEFAULT;
ALTER TABLE webhook_apps ALTER COLUMN id TYPE BIGINT;
DROP SEQUENCE IF EXISTS webhook_apps_id_seq CASCADE;

ALTER TABLE webhook_app_events ALTER COLUMN id DROP DEFAULT;
ALTER TABLE webhook_app_events ALTER COLUMN id TYPE BIGINT;
DROP SEQUENCE IF EXISTS webhook_app_events_id_seq CASCADE;

ALTER TABLE webhook_endpoints ALTER COLUMN id DROP DEFAULT;
ALTER TABLE webhook_endpoints ALTER COLUMN id TYPE BIGINT;
DROP SEQUENCE IF EXISTS webhook_endpoints_id_seq CASCADE;

ALTER TABLE active_webhook_deliveries ALTER COLUMN id DROP DEFAULT;
ALTER TABLE active_webhook_deliveries ALTER COLUMN id TYPE BIGINT;
DROP SEQUENCE IF EXISTS active_webhook_deliveries_id_seq CASCADE;

ALTER TABLE api_key_apps ALTER COLUMN id DROP DEFAULT;
ALTER TABLE api_key_apps ALTER COLUMN id TYPE BIGINT;
DROP SEQUENCE IF EXISTS api_key_apps_id_seq CASCADE;

ALTER TABLE api_keys ALTER COLUMN id DROP DEFAULT;
ALTER TABLE api_keys ALTER COLUMN id TYPE BIGINT;
DROP SEQUENCE IF EXISTS api_keys_id_seq CASCADE;

ALTER TABLE api_key_scopes ALTER COLUMN id DROP DEFAULT;
ALTER TABLE api_key_scopes ALTER COLUMN id TYPE BIGINT;
DROP SEQUENCE IF EXISTS api_key_scopes_id_seq CASCADE;