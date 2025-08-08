-- Update hybrid search functions to work with halfvec(3072) embeddings

-- Drop existing functions
DROP FUNCTION IF EXISTS hybrid_search_kb_enhanced;
DROP FUNCTION IF EXISTS hybrid_search_memories;

-- Only recreate hybrid_search_memories since hybrid_search_kb_enhanced is not used
CREATE OR REPLACE FUNCTION hybrid_search_memories(
    p_query_text TEXT,
    p_query_embedding vector(3072),
    p_agent_id BIGINT,
    p_context_id BIGINT,
    p_max_results INT DEFAULT 10,
    p_min_relevance double precision DEFAULT 0.0,
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
    created_at TIMESTAMP WITH TIME ZONE
) AS $$
BEGIN
    RETURN QUERY
    WITH 
    -- Vector search
    vector_search AS (
        SELECT 
            m.id,
            m.content,
            m.memory_type,
            m.importance,
            m.created_at,
            1.0 - (m.embedding::vector(3072) <-> p_query_embedding) AS similarity
        FROM memories m
        WHERE m.agent_id = p_agent_id
          AND m.context_id = p_context_id
          AND m.embedding IS NOT NULL
          AND (1.0 - (m.embedding::vector(3072) <-> p_query_embedding)) >= p_min_relevance
        ORDER BY similarity DESC
        LIMIT p_max_results * 2
    ),
    -- Text search
    text_search AS (
        SELECT 
            m.id,
            ts_rank(to_tsvector('english', m.content), plainto_tsquery('english', p_query_text)) AS rank
        FROM memories m
        WHERE m.agent_id = p_agent_id
          AND m.context_id = p_context_id
          AND to_tsvector('english', m.content) @@ plainto_tsquery('english', p_query_text)
        ORDER BY rank DESC
        LIMIT p_max_results * 2
    ),
    -- Combine results
    combined AS (
        SELECT 
            COALESCE(vs.id, ts.id) AS id,
            vs.content,
            vs.memory_type,
            vs.importance,
            vs.created_at,
            COALESCE(vs.similarity, 0.0) AS vector_similarity,
            COALESCE(ts.rank, 0.0)::double precision AS text_rank,
            (p_vector_weight * COALESCE(vs.similarity, 0.0) + 
             p_text_weight * COALESCE(ts.rank, 0.0)) AS combined_score
        FROM vector_search vs
        FULL OUTER JOIN text_search ts ON vs.id = ts.id
    )
    SELECT 
        c.id,
        c.content,
        c.memory_type,
        c.importance,
        c.vector_similarity,
        c.text_rank,
        c.combined_score,
        c.created_at
    FROM combined c
    WHERE c.combined_score >= p_min_relevance
    ORDER BY c.combined_score DESC
    LIMIT p_max_results;
END;
$$ LANGUAGE plpgsql;