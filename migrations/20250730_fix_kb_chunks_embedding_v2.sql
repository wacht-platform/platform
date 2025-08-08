-- Fix knowledge_base_document_chunks embedding column
-- This handles the case where existing embeddings are 768 dimensions

-- Drop the existing index
DROP INDEX IF EXISTS kb_doc_chunks_embedding_idx;

-- Since we can't convert 768 dim vectors to 3072 dim directly,
-- we need to temporarily allow NULL and clear existing embeddings
ALTER TABLE knowledge_base_document_chunks 
ALTER COLUMN embedding DROP NOT NULL;

-- Clear existing 768-dimensional embeddings (they need to be regenerated anyway)
UPDATE knowledge_base_document_chunks SET embedding = NULL;

-- Now change the column type to halfvec(3072)
ALTER TABLE knowledge_base_document_chunks 
ALTER COLUMN embedding TYPE halfvec(3072);

-- Restore NOT NULL constraint after regenerating embeddings
-- (This will be done when documents are reprocessed)

-- Create new index for halfvec type
CREATE INDEX idx_knowledge_base_document_chunks_embedding 
ON knowledge_base_document_chunks USING hnsw(embedding halfvec_l2_ops)
WHERE embedding IS NOT NULL;

-- Add comment
COMMENT ON COLUMN knowledge_base_document_chunks.embedding IS 'Vector embedding using gemini-embedding-001 model (3072 dimensions, halfvec type)';

-- Note: All documents need to be reprocessed to generate new 3072-dimensional embeddings