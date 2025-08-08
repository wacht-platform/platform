-- Remove learning metrics from memories table
-- These columns are being removed as they're not needed in the current design

ALTER TABLE memories 
DROP COLUMN IF EXISTS citation_count,
DROP COLUMN IF EXISTS cross_context_value,
DROP COLUMN IF EXISTS learning_confidence,
DROP COLUMN IF EXISTS relevance_score,
DROP COLUMN IF EXISTS usefulness_score;

-- Drop all the decay-related functions
DROP FUNCTION IF EXISTS calculate_base_decay CASCADE;
DROP FUNCTION IF EXISTS update_access_metrics CASCADE;
DROP FUNCTION IF EXISTS update_updated_at_column CASCADE;

-- Drop the triggers that use these functions
DROP TRIGGER IF EXISTS update_conversations_updated_at ON conversations;
DROP TRIGGER IF EXISTS update_memories_updated_at ON memories;

-- Update comments
COMMENT ON TABLE memories IS 'Cross-context learnings and insights';
COMMENT ON COLUMN memories.base_temporal_score IS 'Pre-computed decay score based on access patterns and time';