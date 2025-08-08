-- Add indexes to optimize the mixed conversation query for LLM context

-- Index for finding the last execution summary efficiently
-- This supports: WHERE context_id = ? AND message_type = 'execution_summary' ORDER BY id DESC LIMIT 1
CREATE INDEX IF NOT EXISTS idx_conversations_context_summary 
ON conversations(context_id, message_type, id DESC)
WHERE message_type = 'execution_summary';

-- The existing index from the token_count migration should handle the general case:
-- CREATE INDEX idx_conversations_context_token ON conversations(context_id, id DESC, token_count);
-- This already supports: WHERE context_id = ? ORDER BY id

-- Optional: If you want even better performance for the specific query pattern
-- This is a covering index that includes all columns needed by the query
CREATE INDEX IF NOT EXISTS idx_conversations_llm_query 
ON conversations(context_id, id DESC) 
INCLUDE (timestamp, content, message_type, token_count, created_at, updated_at);

-- Add comment to document the indexes
COMMENT ON INDEX idx_conversations_context_summary IS 'Optimizes finding the last execution summary for LLM context queries';
COMMENT ON INDEX idx_conversations_llm_query IS 'Covering index for efficient LLM conversation history queries';