-- Refactor agent_execution_messages table
-- Remove unnecessary columns and add new ones for embeddings

-- Remove columns that are no longer needed
ALTER TABLE agent_execution_messages 
DROP COLUMN IF EXISTS metadata,
DROP COLUMN IF EXISTS tool_calls,
DROP COLUMN IF EXISTS tool_results;

-- Add embedding column for vector search (768 dimensions for text-embedding-004)
ALTER TABLE agent_execution_messages 
ADD COLUMN IF NOT EXISTS embedding vector(768);

-- Add extracted_data column to store parsed/extracted information from XML
ALTER TABLE agent_execution_messages 
ADD COLUMN IF NOT EXISTS extracted_data JSONB;

-- Create HNSW index for efficient vector similarity search
DROP INDEX IF EXISTS agent_execution_messages_embedding_idx;
CREATE INDEX agent_execution_messages_embedding_idx 
ON agent_execution_messages 
USING hnsw (embedding vector_l2_ops) 
WHERE embedding IS NOT NULL;

-- Create index on extracted_data for efficient JSON queries
DROP INDEX IF EXISTS idx_agent_execution_messages_extracted_data;
CREATE INDEX idx_agent_execution_messages_extracted_data 
ON agent_execution_messages 
USING gin (extracted_data) 
WHERE extracted_data IS NOT NULL;