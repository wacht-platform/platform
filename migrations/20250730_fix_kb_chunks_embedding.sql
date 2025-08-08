-- Fix knowledge_base_document_chunks embedding column
-- The previous migration only partially succeeded - memories table was updated but not kb chunks

-- Drop the existing index on knowledge_base_document_chunks
DROP INDEX IF EXISTS kb_doc_chunks_embedding_idx;

-- Update knowledge_base_document_chunks table to halfvec type
ALTER TABLE knowledge_base_document_chunks 
ALTER COLUMN embedding TYPE halfvec(3072);

-- Recreate index with halfvec operator class
CREATE INDEX idx_knowledge_base_document_chunks_embedding 
ON knowledge_base_document_chunks USING hnsw(embedding halfvec_l2_ops);

-- Add comment
COMMENT ON COLUMN knowledge_base_document_chunks.embedding IS 'Vector embedding using gemini-embedding-001 model (3072 dimensions, halfvec type)';

-- Note: Existing embeddings in this table will need to be regenerated
-- as the dimensions have changed from 768 to 3072