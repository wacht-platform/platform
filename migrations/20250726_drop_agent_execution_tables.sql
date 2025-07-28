-- Drop agent_execution_messages and agent_execution_memories tables and related objects
-- These tables are no longer needed as we're moving to a different memory/messaging architecture

-- Drop functions that use agent_execution_memories table
DROP FUNCTION IF EXISTS hybrid_search_memories CASCADE;

-- Drop triggers first
DROP TRIGGER IF EXISTS trig_update_memories_search_vector ON agent_execution_memories;
DROP TRIGGER IF EXISTS update_agent_execution_messages_search_vector_trigger ON agent_execution_messages;
DROP TRIGGER IF EXISTS update_agent_execution_messages_embedding_trigger ON agent_execution_messages;

-- Drop trigger functions
DROP FUNCTION IF EXISTS update_search_vector_memories();
DROP FUNCTION IF EXISTS update_agent_execution_messages_search_vector();
DROP FUNCTION IF EXISTS generate_embedding_for_message();

-- Drop indexes for agent_execution_memories
DROP INDEX IF EXISTS idx_agent_execution_memories_search_vector;
DROP INDEX IF EXISTS idx_agent_execution_memories_content_trgm;
DROP INDEX IF EXISTS idx_agent_execution_memories_embedding;
DROP INDEX IF EXISTS idx_agent_execution_memories_agent_id;
DROP INDEX IF EXISTS idx_agent_execution_memories_context_id;
DROP INDEX IF EXISTS idx_agent_execution_memories_created_at;
DROP INDEX IF EXISTS idx_memories_agent_created;
DROP INDEX IF EXISTS idx_memories_type;
DROP INDEX IF EXISTS idx_memories_importance;

-- Drop indexes for agent_execution_messages  
DROP INDEX IF EXISTS idx_agent_execution_messages_execution_context_id;
DROP INDEX IF EXISTS idx_agent_execution_messages_created_at;
DROP INDEX IF EXISTS idx_agent_execution_messages_role;
DROP INDEX IF EXISTS idx_agent_execution_messages_embedding;
DROP INDEX IF EXISTS idx_agent_execution_messages_search_vector;

-- Drop the tables (CASCADE will handle foreign key constraints)
DROP TABLE IF EXISTS agent_execution_memories CASCADE;
DROP TABLE IF EXISTS agent_execution_messages CASCADE;