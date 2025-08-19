-- Remove grouping and deduplication fields from notifications table
-- =====================================================

-- Drop indexes first
DROP INDEX IF EXISTS idx_notifications_dedupe;
DROP INDEX IF EXISTS idx_notifications_group;

-- Remove columns
ALTER TABLE notifications 
    DROP COLUMN IF EXISTS group_id,
    DROP COLUMN IF EXISTS group_count,
    DROP COLUMN IF EXISTS dedupe_key,
    DROP COLUMN IF EXISTS source,
    DROP COLUMN IF EXISTS source_id;