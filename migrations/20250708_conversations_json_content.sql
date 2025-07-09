-- Migration: Convert conversations to JSONB and remove embedding and scores
-- WARNING: This will delete all existing conversation data

-- Delete all existing data from conversations table
TRUNCATE TABLE conversations;

-- Drop any existing indexes on the content column
-- This will find and drop all indexes that use the content column
DO $$
DECLARE
    idx_name TEXT;
BEGIN
    FOR idx_name IN 
        SELECT indexname 
        FROM pg_indexes 
        WHERE tablename = 'conversations' 
        AND indexdef LIKE '%content%'
    LOOP
        EXECUTE 'DROP INDEX IF EXISTS ' || idx_name;
    END LOOP;
END $$;

-- Drop the embedding column
ALTER TABLE conversations
DROP COLUMN IF EXISTS embedding;

-- Drop all score-related columns
ALTER TABLE conversations
DROP COLUMN IF EXISTS base_temporal_score,
DROP COLUMN IF EXISTS access_count,
DROP COLUMN IF EXISTS first_accessed_at,
DROP COLUMN IF EXISTS last_accessed_at,
DROP COLUMN IF EXISTS citation_count,
DROP COLUMN IF EXISTS relevance_score,
DROP COLUMN IF EXISTS usefulness_score,
DROP COLUMN IF EXISTS compression_level,
DROP COLUMN IF EXISTS compressed_content;

-- Change content column type from TEXT to JSONB
ALTER TABLE conversations
ALTER COLUMN content TYPE JSONB USING content::JSONB;

-- Update the message_type check constraint to include new types
ALTER TABLE conversations
DROP CONSTRAINT IF EXISTS conversations_message_type_check;

ALTER TABLE conversations
ADD CONSTRAINT conversations_message_type_check
CHECK (message_type IN ('user_message', 'agent_response', 'assistant_acknowledgment', 'assistant_ideation', 'assistant_task_execution', 'assistant_validation'));
