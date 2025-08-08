-- Update vector dimensions from 768 to 3072 for gemini-embedding-001 model
-- This migration updates embedding columns to use halfvec type which supports up to 4000 dimensions

-- Drop indexes before changing column types
DROP INDEX IF EXISTS idx_memories_embedding;
DROP INDEX IF EXISTS idx_knowledge_base_document_chunks_embedding;

-- Update memories table to halfvec type
ALTER TABLE memories 
ALTER COLUMN embedding TYPE halfvec(3072);

-- Update knowledge_base_document_chunks table to halfvec type
ALTER TABLE knowledge_base_document_chunks 
ALTER COLUMN embedding TYPE halfvec(3072);

-- Recreate indexes with new dimensions using halfvec operations
CREATE INDEX IF NOT EXISTS idx_memories_embedding 
ON memories USING hnsw(embedding halfvec_l2_ops) 
WHERE embedding IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_knowledge_base_document_chunks_embedding 
ON knowledge_base_document_chunks USING hnsw(embedding halfvec_l2_ops);

-- Add comments about the model change
COMMENT ON COLUMN memories.embedding IS 'Vector embedding using gemini-embedding-001 model (3072 dimensions, halfvec type)';
COMMENT ON COLUMN knowledge_base_document_chunks.embedding IS 'Vector embedding using gemini-embedding-001 model (3072 dimensions, halfvec type)';

-- Note: Existing embeddings will need to be regenerated with the new model
-- as the dimensions have changed from 768 to 3072