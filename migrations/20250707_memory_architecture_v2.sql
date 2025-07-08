-- Memory Architecture V2: Conversations and Memories with Decay
-- This migration creates the new 2-table memory architecture with decay scoring

-- Ensure the vector extension is enabled
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm; -- For text similarity

-- Create conversations table for context-bound interactions
CREATE TABLE IF NOT EXISTS conversations (
    id BIGINT PRIMARY KEY,
    context_id BIGINT NOT NULL,
    timestamp TIMESTAMPTZ DEFAULT NOW(),
    content TEXT NOT NULL,
    embedding vector(768),
    message_type TEXT NOT NULL CHECK (message_type IN ('user_message', 'agent_response')),
    
    -- Decay components
    base_temporal_score FLOAT DEFAULT 1.0,
    access_count INT DEFAULT 0,
    first_accessed_at TIMESTAMPTZ DEFAULT NOW(),
    last_accessed_at TIMESTAMPTZ DEFAULT NOW(),
    
    -- Learning metrics from LLM citations
    citation_count INT DEFAULT 0,
    relevance_score FLOAT DEFAULT 0.0,
    usefulness_score FLOAT DEFAULT 0.0,
    
    -- Compression support
    compression_level INT DEFAULT 0, -- 0=none, 1=summary, 2=keywords
    compressed_content TEXT,
    
    -- Metadata
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Create memories table for cross-context learnings
CREATE TABLE IF NOT EXISTS memories (
    id BIGINT PRIMARY KEY,
    content TEXT NOT NULL,
    embedding vector(768),
    memory_category TEXT NOT NULL CHECK (memory_category IN ('procedural', 'semantic', 'episodic', 'working')),
    
    -- Decay components
    base_temporal_score FLOAT DEFAULT 1.0,
    access_count INT DEFAULT 0,
    first_accessed_at TIMESTAMPTZ DEFAULT NOW(),
    last_accessed_at TIMESTAMPTZ DEFAULT NOW(),
    
    -- Learning metrics from LLM citations
    citation_count INT DEFAULT 0,
    cross_context_value FLOAT DEFAULT 0.0,
    learning_confidence FLOAT DEFAULT 0.5,
    relevance_score FLOAT DEFAULT 0.0,
    usefulness_score FLOAT DEFAULT 0.0,
    
    -- Origin tracking
    creation_context_id BIGINT,
    last_reinforced_at TIMESTAMPTZ DEFAULT NOW(),
    
    -- Enhanced importance scoring
    semantic_centrality FLOAT DEFAULT 0.0, -- How connected to other memories
    uniqueness_score FLOAT DEFAULT 0.0,    -- How rare/unique the information
    
    -- Compression support
    compression_level INT DEFAULT 0,
    compressed_content TEXT,
    
    -- Flexible decay profile for different contexts
    context_decay_profile JSONB DEFAULT '{}',
    
    -- Metadata
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Indexes for conversations table
CREATE INDEX IF NOT EXISTS idx_conversations_context ON conversations(context_id);
CREATE INDEX IF NOT EXISTS idx_conversations_mru ON conversations(last_accessed_at DESC);
CREATE INDEX IF NOT EXISTS idx_conversations_recent ON conversations(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_conversations_embedding ON conversations USING hnsw(embedding vector_l2_ops) WHERE embedding IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_conversations_decay ON conversations(base_temporal_score DESC) WHERE base_temporal_score > 0.1;
CREATE INDEX IF NOT EXISTS idx_conversations_message_type ON conversations(message_type);
CREATE INDEX IF NOT EXISTS idx_conversations_content_trgm ON conversations USING gin(content gin_trgm_ops); -- For text search

-- Indexes for memories table
CREATE INDEX IF NOT EXISTS idx_memories_mru ON memories(last_accessed_at DESC);
CREATE INDEX IF NOT EXISTS idx_memories_embedding ON memories USING hnsw(embedding vector_l2_ops) WHERE embedding IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(memory_category);
CREATE INDEX IF NOT EXISTS idx_memories_decay ON memories(base_temporal_score DESC) WHERE base_temporal_score > 0.1;
CREATE INDEX IF NOT EXISTS idx_memories_creation_context ON memories(creation_context_id);
CREATE INDEX IF NOT EXISTS idx_memories_confidence ON memories(learning_confidence DESC);
CREATE INDEX IF NOT EXISTS idx_memories_centrality ON memories(semantic_centrality DESC);
CREATE INDEX IF NOT EXISTS idx_memories_content_trgm ON memories USING gin(content gin_trgm_ops); -- For text search

-- Create decay calculation function
CREATE OR REPLACE FUNCTION calculate_base_decay(
    access_count INT,
    citation_count INT,
    first_accessed TIMESTAMPTZ,
    last_accessed TIMESTAMPTZ,
    relevance_score FLOAT DEFAULT 0.0,
    usefulness_score FLOAT DEFAULT 0.0,
    compression_level INT DEFAULT 0
) RETURNS FLOAT AS $$
DECLARE
    now_ts FLOAT := EXTRACT(EPOCH FROM NOW());
    first_ts FLOAT := EXTRACT(EPOCH FROM first_accessed);
    last_ts FLOAT := EXTRACT(EPOCH FROM last_accessed);
    hours_since_last FLOAT := (now_ts - last_ts) / 3600.0;
    days_since_first FLOAT := (now_ts - first_ts) / (3600.0 * 24.0);
    
    -- Base decay components
    recency_score FLOAT := EXP(-hours_since_last / 168.0); -- 1 week half-life
    maturity_score FLOAT := LEAST(days_since_first / 30.0, 1.0); -- Cap at 30 days
    usage_score FLOAT := LOG(access_count + 1) / 10.0;
    citation_score FLOAT := LOG(citation_count + 1) / 5.0;
    learned_score FLOAT := (relevance_score + usefulness_score) / 2.0;
    
    -- Compression penalty (compressed content decays faster)
    compression_penalty FLOAT := 1.0 - (compression_level * 0.2);
BEGIN
    -- Ensure no negative or zero scores
    RETURN GREATEST(0.001, recency_score * maturity_score * usage_score * citation_score * (0.5 + learned_score) * compression_penalty);
END;
$$ LANGUAGE plpgsql IMMUTABLE;

-- Triggers to update updated_at timestamp
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

DROP TRIGGER IF EXISTS update_conversations_updated_at ON conversations;
CREATE TRIGGER update_conversations_updated_at 
    BEFORE UPDATE ON conversations 
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

DROP TRIGGER IF EXISTS update_memories_updated_at ON memories;
CREATE TRIGGER update_memories_updated_at 
    BEFORE UPDATE ON memories 
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Function to update decay scores on access
CREATE OR REPLACE FUNCTION update_access_metrics()
RETURNS TRIGGER AS $$
BEGIN
    NEW.access_count = OLD.access_count + 1;
    NEW.last_accessed_at = NOW();
    NEW.base_temporal_score = calculate_base_decay(
        NEW.access_count,
        NEW.citation_count,
        NEW.first_accessed_at,
        NEW.last_accessed_at,
        NEW.relevance_score,
        NEW.usefulness_score,
        NEW.compression_level
    );
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Memory boundaries configuration table
CREATE TABLE IF NOT EXISTS memory_boundaries (
    context_id BIGINT PRIMARY KEY,
    max_conversations INT DEFAULT 1000,
    max_memories_per_category JSONB DEFAULT '{"procedural": 500, "semantic": 500, "episodic": 500}',
    compression_threshold_days INT DEFAULT 30,
    eviction_threshold_score FLOAT DEFAULT 0.05,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Comments for documentation
COMMENT ON TABLE conversations IS 'Context-bound interactions with decay-based relevance scoring';
COMMENT ON TABLE memories IS 'Cross-context learnings and insights with citation-based reinforcement';
COMMENT ON TABLE memory_boundaries IS 'Per-context memory limits and eviction policies';

COMMENT ON COLUMN conversations.base_temporal_score IS 'Pre-computed decay score updated on access';
COMMENT ON COLUMN conversations.citation_count IS 'Number of times LLM cited this conversation';
COMMENT ON COLUMN conversations.compression_level IS '0=full text, 1=summarized, 2=keywords only';

COMMENT ON COLUMN memories.cross_context_value IS 'How valuable this memory is across different contexts';
COMMENT ON COLUMN memories.learning_confidence IS 'Confidence level in this learning (0.0-1.0)';
COMMENT ON COLUMN memories.semantic_centrality IS 'How connected this memory is to other memories';
COMMENT ON COLUMN memories.uniqueness_score IS 'How rare or unique this information is';
COMMENT ON COLUMN memories.context_decay_profile IS 'JSON map of context_id to custom decay parameters';

-- Verify installation with a simple test
DO $$
DECLARE
    test_score FLOAT;
BEGIN
    -- Test the decay calculation function
    test_score := calculate_base_decay(
        10,                           -- access_count
        5,                            -- citation_count
        NOW() - INTERVAL '7 days',    -- first_accessed
        NOW() - INTERVAL '1 hour',    -- last_accessed
        0.8,                          -- relevance_score
        0.7,                          -- usefulness_score
        0                             -- compression_level
    );
    
    IF test_score IS NULL OR test_score <= 0 THEN
        RAISE EXCEPTION 'Decay calculation test failed: score=%', test_score;
    END IF;
    
    RAISE NOTICE 'Memory V2 migration completed successfully. Test decay score: %', test_score;
END $$;