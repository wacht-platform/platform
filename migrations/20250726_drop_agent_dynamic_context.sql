-- Drop agent_dynamic_context table and related objects
-- This table is no longer needed as dynamic context functionality has been removed

-- Drop triggers first
DROP TRIGGER IF EXISTS trig_update_dynamic_context_search_vector ON agent_dynamic_context;
DROP FUNCTION IF EXISTS update_search_vector_dynamic_context();

-- Drop indexes (excluding primary key which will be dropped with the table)
DROP INDEX IF EXISTS agent_dynamic_context_embedding_idx;
DROP INDEX IF EXISTS idx_agent_dynamic_context_execution_id;
DROP INDEX IF EXISTS idx_dynamic_context_content_trgm;
DROP INDEX IF EXISTS idx_dynamic_context_search_vector;

-- Drop the table (CASCADE will handle the foreign key constraint)
DROP TABLE IF EXISTS agent_dynamic_context CASCADE;