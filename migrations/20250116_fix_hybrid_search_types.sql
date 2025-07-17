-- Migration: Fix hybrid search function return types
-- Date: 2025-01-16
-- Description: Fixes type mismatch between PostgreSQL FLOAT and Rust f64 by using double precision

-- Drop existing functions
DROP FUNCTION IF EXISTS hybrid_search_kb_enhanced(TEXT, vector(768), BIGINT, BIGINT, INT, FLOAT, FLOAT, FLOAT);
DROP FUNCTION IF EXISTS hybrid_search_memories(TEXT, vector(768), BIGINT, BIGINT, INT, FLOAT, FLOAT, FLOAT);

-- Recreate hybrid_search_kb_enhanced with double precision types
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
            (1.0 - (kbc.embedding <-> p_query_embedding))::double precision AS similarity
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
            ts_rank(kbc.search_vector, plainto_tsquery('english', p_query_text))::double precision AS rank
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
            ts_rank(d.search_vector, plainto_tsquery('english', p_query_text))::double precision AS doc_rank,
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
            cvs.similarity,
            COALESCE(cts.rank, 0.0::double precision) AS chunk_text_rank,
            COALESCE(dts.doc_rank, 0.0::double precision) AS doc_text_rank,
            -- Boost score if document title/description matches
            (p_vector_weight * cvs.similarity + 
             p_text_weight * LEAST((COALESCE(cts.rank, 0.0) + COALESCE(dts.doc_rank, 0.0) * 2) * 10, 1.0))::double precision AS combined_score,
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
            COALESCE(cvs.similarity, 0.0::double precision) AS similarity,
            cts.rank AS chunk_text_rank,
            COALESCE(dts.doc_rank, 0.0::double precision) AS doc_text_rank,
            (p_vector_weight * COALESCE(cvs.similarity, 0.0) + 
             p_text_weight * LEAST((cts.rank + COALESCE(dts.doc_rank, 0.0) * 2) * 10, 1.0))::double precision AS combined_score,
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
        c.similarity::double precision AS vector_similarity,
        (c.chunk_text_rank + c.doc_text_rank)::double precision AS text_rank,
        c.combined_score::double precision
    FROM combined c
    LEFT JOIN ai_knowledge_base_documents d ON c.document_id = d.id
    WHERE c.combined_score >= p_min_relevance * p_vector_weight
    ORDER BY c.document_id, c.chunk_index, c.combined_score DESC
    LIMIT p_max_results;
END;
$$ LANGUAGE plpgsql;

-- Recreate hybrid_search_memories with double precision types
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
            (1.0 - (m.embedding <-> p_query_embedding))::double precision AS similarity,
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
            ts_rank(m.search_vector, plainto_tsquery('english', p_query_text))::double precision AS rank
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
            vs.similarity,
            COALESCE(ts.rank, 0.0::double precision) AS text_rank,
            (p_vector_weight * vs.similarity + p_text_weight * LEAST(ts.rank * 10, 1.0))::double precision AS combined_score,
            vs.created_at
        FROM vector_search vs
        LEFT JOIN text_search ts ON vs.id = ts.id
        UNION
        SELECT 
            m.id,
            m.content,
            m.memory_type::TEXT,
            m.importance,
            COALESCE(vs.similarity, 0.0::double precision) AS similarity,
            ts.rank,
            (p_vector_weight * COALESCE(vs.similarity, 0.0) + p_text_weight * LEAST(ts.rank * 10, 1.0))::double precision AS combined_score,
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
        combined_score,
        created_at
    FROM combined
    WHERE combined_score >= p_min_relevance * p_vector_weight
    ORDER BY id, combined_score DESC
    LIMIT p_max_results;
END;
$$ LANGUAGE plpgsql;

-- Add comment explaining the fix
COMMENT ON FUNCTION hybrid_search_kb_enhanced IS 'Fixed version with double precision types to match Rust f64 expectations';
COMMENT ON FUNCTION hybrid_search_memories IS 'Fixed version with double precision types to match Rust f64 expectations';