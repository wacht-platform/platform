-- Migration: Add full-text search support for hybrid search
-- Date: 2025-01-15
-- Description: Adds full-text search columns, indexes, and functions to enable hybrid vector + text search

-- Enable required extensions if not already enabled
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Add full-text search columns to knowledge_base_document_chunks
ALTER TABLE knowledge_base_document_chunks 
ADD COLUMN IF NOT EXISTS search_vector tsvector;

-- Add full-text search columns to agent_execution_memories
ALTER TABLE agent_execution_memories 
ADD COLUMN IF NOT EXISTS search_vector tsvector;

-- Add full-text search columns to agent_dynamic_context
ALTER TABLE agent_dynamic_context 
ADD COLUMN IF NOT EXISTS search_vector tsvector;

-- Add full-text search columns to ai_knowledge_base_documents
ALTER TABLE ai_knowledge_base_documents 
ADD COLUMN IF NOT EXISTS search_vector tsvector;

-- Create function to generate search vector from content
CREATE OR REPLACE FUNCTION generate_search_vector(content TEXT) 
RETURNS tsvector AS $$
BEGIN
    -- Create weighted tsvector: 
    -- 'A' weight for first 100 chars (title/summary)
    -- 'B' weight for rest of content
    RETURN setweight(to_tsvector('english', COALESCE(LEFT(content, 100), '')), 'A') ||
           setweight(to_tsvector('english', COALESCE(content, '')), 'B');
END;
$$ LANGUAGE plpgsql IMMUTABLE;

-- Create function to generate weighted search vector for documents
CREATE OR REPLACE FUNCTION generate_document_search_vector(title TEXT, description TEXT) 
RETURNS tsvector AS $$
BEGIN
    -- Create weighted tsvector: 
    -- 'A' weight for title (highest priority)
    -- 'B' weight for description
    RETURN setweight(to_tsvector('english', COALESCE(title, '')), 'A') ||
           setweight(to_tsvector('english', COALESCE(description, '')), 'B');
END;
$$ LANGUAGE plpgsql IMMUTABLE;

-- Update existing rows with search vectors
UPDATE knowledge_base_document_chunks 
SET search_vector = generate_search_vector(content)
WHERE search_vector IS NULL;

UPDATE agent_execution_memories 
SET search_vector = generate_search_vector(content)
WHERE search_vector IS NULL;

UPDATE agent_dynamic_context 
SET search_vector = generate_search_vector(content)
WHERE search_vector IS NULL;

UPDATE ai_knowledge_base_documents 
SET search_vector = generate_document_search_vector(title, description)
WHERE search_vector IS NULL;

-- Create GIN indexes for full-text search
CREATE INDEX IF NOT EXISTS idx_kb_chunks_search_vector 
ON knowledge_base_document_chunks USING GIN(search_vector);

CREATE INDEX IF NOT EXISTS idx_memories_search_vector 
ON agent_execution_memories USING GIN(search_vector);

CREATE INDEX IF NOT EXISTS idx_dynamic_context_search_vector 
ON agent_dynamic_context USING GIN(search_vector);

CREATE INDEX IF NOT EXISTS idx_kb_documents_search_vector 
ON ai_knowledge_base_documents USING GIN(search_vector);

-- Create trigram indexes for fuzzy text search
CREATE INDEX IF NOT EXISTS idx_kb_chunks_content_trgm 
ON knowledge_base_document_chunks USING GIN(content gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_memories_content_trgm 
ON agent_execution_memories USING GIN(content gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_dynamic_context_content_trgm 
ON agent_dynamic_context USING GIN(content gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_kb_documents_title_trgm 
ON ai_knowledge_base_documents USING GIN(title gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_kb_documents_description_trgm 
ON ai_knowledge_base_documents USING GIN(description gin_trgm_ops);

-- Create triggers to automatically update search vectors
CREATE OR REPLACE FUNCTION update_search_vector_kb_chunks() 
RETURNS TRIGGER AS $$
BEGIN
    NEW.search_vector := generate_search_vector(NEW.content);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION update_search_vector_memories() 
RETURNS TRIGGER AS $$
BEGIN
    NEW.search_vector := generate_search_vector(NEW.content);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION update_search_vector_dynamic_context() 
RETURNS TRIGGER AS $$
BEGIN
    NEW.search_vector := generate_search_vector(NEW.content);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION update_search_vector_kb_documents() 
RETURNS TRIGGER AS $$
BEGIN
    NEW.search_vector := generate_document_search_vector(NEW.title, NEW.description);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Drop existing triggers if they exist
DROP TRIGGER IF EXISTS trig_update_kb_chunks_search_vector ON knowledge_base_document_chunks;
DROP TRIGGER IF EXISTS trig_update_memories_search_vector ON agent_execution_memories;
DROP TRIGGER IF EXISTS trig_update_dynamic_context_search_vector ON agent_dynamic_context;
DROP TRIGGER IF EXISTS trig_update_kb_documents_search_vector ON ai_knowledge_base_documents;

-- Create triggers
CREATE TRIGGER trig_update_kb_chunks_search_vector 
BEFORE INSERT OR UPDATE OF content ON knowledge_base_document_chunks
FOR EACH ROW EXECUTE FUNCTION update_search_vector_kb_chunks();

CREATE TRIGGER trig_update_memories_search_vector 
BEFORE INSERT OR UPDATE OF content ON agent_execution_memories
FOR EACH ROW EXECUTE FUNCTION update_search_vector_memories();

CREATE TRIGGER trig_update_dynamic_context_search_vector 
BEFORE INSERT OR UPDATE OF content ON agent_dynamic_context
FOR EACH ROW EXECUTE FUNCTION update_search_vector_dynamic_context();

CREATE TRIGGER trig_update_kb_documents_search_vector 
BEFORE INSERT OR UPDATE OF title, description ON ai_knowledge_base_documents
FOR EACH ROW EXECUTE FUNCTION update_search_vector_kb_documents();

-- Create enhanced hybrid search function that includes document metadata
CREATE OR REPLACE FUNCTION hybrid_search_kb_enhanced(
    p_query_text TEXT,
    p_query_embedding vector(768),
    p_knowledge_base_id BIGINT,
    p_deployment_id BIGINT,
    p_max_results INT DEFAULT 10,
    p_min_relevance double precision DEFAULT 0.7,
    p_vector_weight double precision DEFAULT 0.7,
    p_text_weight double precision DEFAULT 0.3
) RETURNS TABLE (
    document_id BIGINT,
    chunk_index INT,
    content TEXT,
    document_title TEXT,
    document_description TEXT,
    vector_similarity double precision,
    text_rank double precision,
    combined_score double precision
) AS $$
BEGIN
    RETURN QUERY
    WITH 
    -- Search in document chunks (vector)
    chunk_vector_search AS (
        SELECT 
            kbc.document_id,
            kbc.chunk_index,
            kbc.content,
            1.0 - (kbc.embedding <-> p_query_embedding) AS similarity
        FROM knowledge_base_document_chunks kbc
        WHERE kbc.knowledge_base_id = p_knowledge_base_id
          AND kbc.deployment_id = p_deployment_id
          AND (1.0 - (kbc.embedding <-> p_query_embedding)) >= p_min_relevance
        ORDER BY similarity DESC
        LIMIT p_max_results * 2
    ),
    -- Search in document chunks (text)
    chunk_text_search AS (
        SELECT 
            kbc.document_id,
            kbc.chunk_index,
            ts_rank(kbc.search_vector, plainto_tsquery('english', p_query_text)) AS rank
        FROM knowledge_base_document_chunks kbc
        WHERE kbc.knowledge_base_id = p_knowledge_base_id
          AND kbc.deployment_id = p_deployment_id
          AND kbc.search_vector @@ plainto_tsquery('english', p_query_text)
        ORDER BY rank DESC
        LIMIT p_max_results * 2
    ),
    -- Search in document titles and descriptions (text)
    doc_text_search AS (
        SELECT 
            d.id AS document_id,
            ts_rank(d.search_vector, plainto_tsquery('english', p_query_text)) AS doc_rank,
            d.title,
            d.description
        FROM ai_knowledge_base_documents d
        WHERE d.knowledge_base_id = p_knowledge_base_id
          AND d.search_vector @@ plainto_tsquery('english', p_query_text)
    ),
    -- Combine all search results
    combined AS (
        SELECT 
            cvs.document_id,
            cvs.chunk_index,
            cvs.content,
            cvs.similarity::double precision,
            COALESCE(cts.rank, 0.0)::double precision AS chunk_text_rank,
            COALESCE(dts.doc_rank, 0.0)::double precision AS doc_text_rank,
            -- Boost score if document title/description matches
            (p_vector_weight * cvs.similarity + 
             p_text_weight * LEAST((COALESCE(cts.rank, 0.0) + COALESCE(dts.doc_rank, 0.0) * 2) * 10, 1.0)) AS combined_score,
            dts.title AS doc_title,
            dts.description AS doc_description
        FROM chunk_vector_search cvs
        LEFT JOIN chunk_text_search cts ON cvs.document_id = cts.document_id AND cvs.chunk_index = cts.chunk_index
        LEFT JOIN doc_text_search dts ON cvs.document_id = dts.document_id
        
        UNION
        
        -- Include text-only matches
        SELECT 
            kbc.document_id,
            kbc.chunk_index,
            kbc.content,
            COALESCE(cvs.similarity, 0.0)::double precision AS similarity,
            cts.rank::double precision AS chunk_text_rank,
            COALESCE(dts.doc_rank, 0.0)::double precision AS doc_text_rank,
            (p_vector_weight * COALESCE(cvs.similarity, 0.0) + 
             p_text_weight * LEAST((cts.rank + COALESCE(dts.doc_rank, 0.0) * 2) * 10, 1.0)) AS combined_score,
            dts.title AS doc_title,
            dts.description AS doc_description
        FROM chunk_text_search cts
        JOIN knowledge_base_document_chunks kbc ON kbc.document_id = cts.document_id AND kbc.chunk_index = cts.chunk_index
        LEFT JOIN chunk_vector_search cvs ON cvs.document_id = cts.document_id AND cvs.chunk_index = cts.chunk_index
        LEFT JOIN doc_text_search dts ON cts.document_id = dts.document_id
        WHERE cvs.document_id IS NULL
    )
    SELECT DISTINCT ON (c.document_id, c.chunk_index)
        c.document_id,
        c.chunk_index,
        c.content,
        COALESCE(c.doc_title, d.title) AS document_title,
        COALESCE(c.doc_description, d.description) AS document_description,
        c.similarity AS vector_similarity,
        c.chunk_text_rank + c.doc_text_rank AS text_rank,
        c.combined_score::double precision
    FROM combined c
    LEFT JOIN ai_knowledge_base_documents d ON c.document_id = d.id
    WHERE c.combined_score >= p_min_relevance * p_vector_weight
    ORDER BY c.document_id, c.chunk_index, c.combined_score DESC
    LIMIT p_max_results;
END;
$$ LANGUAGE plpgsql;

-- Create similar hybrid search functions for memories and dynamic context
CREATE OR REPLACE FUNCTION hybrid_search_memories(
    p_query_text TEXT,
    p_query_embedding vector(768),
    p_agent_id BIGINT,
    p_context_id BIGINT,
    p_max_results INT DEFAULT 10,
    p_min_relevance double precision DEFAULT 0.7,
    p_vector_weight double precision DEFAULT 0.7,
    p_text_weight double precision DEFAULT 0.3
) RETURNS TABLE (
    id BIGINT,
    content TEXT,
    memory_type TEXT,
    importance double precision,
    vector_similarity double precision,
    text_rank double precision,
    combined_score double precision,
    created_at TIMESTAMPTZ
) AS $$
BEGIN
    RETURN QUERY
    WITH vector_search AS (
        SELECT 
            m.id,
            m.content,
            m.memory_type::TEXT,
            m.importance,
            1.0 - (m.embedding <-> p_query_embedding) AS similarity,
            m.created_at
        FROM agent_execution_memories m
        WHERE m.agent_id = p_agent_id
          AND m.execution_context_id = p_context_id
          AND (1.0 - (m.embedding <-> p_query_embedding)) >= p_min_relevance
        ORDER BY similarity DESC
        LIMIT p_max_results * 2
    ),
    text_search AS (
        SELECT 
            m.id,
            ts_rank(m.search_vector, plainto_tsquery('english', p_query_text)) AS rank
        FROM agent_execution_memories m
        WHERE m.agent_id = p_agent_id
          AND m.execution_context_id = p_context_id
          AND m.search_vector @@ plainto_tsquery('english', p_query_text)
        ORDER BY rank DESC
        LIMIT p_max_results * 2
    ),
    combined AS (
        SELECT 
            vs.id,
            vs.content,
            vs.memory_type,
            vs.importance,
            vs.similarity::double precision,
            COALESCE(ts.rank, 0.0)::double precision AS text_rank,
            (p_vector_weight * vs.similarity + p_text_weight * LEAST(ts.rank * 10, 1.0)) AS combined_score,
            vs.created_at
        FROM vector_search vs
        LEFT JOIN text_search ts ON vs.id = ts.id
        UNION
        SELECT 
            m.id,
            m.content,
            m.memory_type::TEXT,
            m.importance,
            COALESCE(vs.similarity, 0.0)::double precision AS similarity,
            ts.rank::double precision,
            (p_vector_weight * COALESCE(vs.similarity, 0.0) + p_text_weight * LEAST(ts.rank * 10, 1.0)) AS combined_score,
            m.created_at
        FROM text_search ts
        JOIN agent_execution_memories m ON m.id = ts.id
        LEFT JOIN vector_search vs ON vs.id = ts.id
        WHERE vs.id IS NULL
    )
    SELECT DISTINCT ON (id)
        id,
        content,
        memory_type,
        importance,
        similarity AS vector_similarity,
        text_rank,
        combined_score::double precision,
        created_at
    FROM combined
    WHERE combined_score >= p_min_relevance * p_vector_weight
    ORDER BY id, combined_score DESC
    LIMIT p_max_results;
END;
$$ LANGUAGE plpgsql;

-- Add comment explaining the migration
COMMENT ON SCHEMA public IS 'Hybrid search migration adds full-text search capabilities alongside existing vector search for improved search quality';