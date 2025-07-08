-- This script contains schema changes for migrating agent-related tables and creating new ones.
-- It is designed to be idempotent and can be safely run multiple times.

-- Ensure the vector extension is enabled
CREATE EXTENSION IF NOT EXISTS vector;

-- Drop existing tables in reverse dependency order (to handle foreign key constraints)
DROP TABLE IF EXISTS knowledge_base_document_chunks CASCADE;
DROP TABLE IF EXISTS agent_dynamic_context CASCADE;
DROP TABLE IF EXISTS agent_execution_memories CASCADE;

-- Handle renaming of existing tables
-- First check if old tables exist and new tables don't exist, then rename
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'ai_execution_contexts') 
       AND NOT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'agent_execution_contexts') THEN
        ALTER TABLE ai_execution_contexts RENAME TO agent_execution_contexts;
    END IF;
    
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'ai_execution_messages') 
       AND NOT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'agent_execution_messages') THEN
        ALTER TABLE ai_execution_messages RENAME TO agent_execution_messages;
    END IF;
END $$;

-- Create the table for agent memories
CREATE TABLE IF NOT EXISTS agent_execution_memories (
    id BIGINT PRIMARY KEY,
    deployment_id BIGINT NOT NULL,
    agent_id BIGINT NOT NULL,
    execution_context_id BIGINT,
    memory_type TEXT NOT NULL,
    content TEXT NOT NULL,
    embedding vector(768) NOT NULL,
    importance REAL NOT NULL,
    access_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_accessed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fk_execution_context
      FOREIGN KEY(execution_context_id)
      REFERENCES agent_execution_contexts(id)
      ON DELETE CASCADE
);

-- Create indexes for the agent memories table
DROP INDEX IF EXISTS idx_agent_execution_memories_agent_id;
CREATE INDEX idx_agent_execution_memories_agent_id ON agent_execution_memories(agent_id);

DROP INDEX IF EXISTS idx_agent_execution_memories_execution_context_id;
CREATE INDEX idx_agent_execution_memories_execution_context_id ON agent_execution_memories(execution_context_id);

-- Naming the HNSW index for idempotency
DROP INDEX IF EXISTS agent_execution_memories_embedding_idx;
CREATE INDEX agent_execution_memories_embedding_idx ON agent_execution_memories USING hnsw (embedding vector_l2_ops);


-- Create the table for agent dynamic context
CREATE TABLE IF NOT EXISTS agent_dynamic_context (
    id BIGINT PRIMARY KEY,
    execution_context_id BIGINT NOT NULL,
    content TEXT NOT NULL,
    source VARCHAR(255),
    embedding vector(768),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fk_execution_context
      FOREIGN KEY(execution_context_id)
      REFERENCES agent_execution_contexts(id)
      ON DELETE CASCADE
);

-- Create indexes for the dynamic context table
DROP INDEX IF EXISTS idx_agent_dynamic_context_execution_id;
CREATE INDEX idx_agent_dynamic_context_execution_id ON agent_dynamic_context(execution_context_id);

-- Naming the HNSW index for idempotency
DROP INDEX IF EXISTS agent_dynamic_context_embedding_idx;
CREATE INDEX agent_dynamic_context_embedding_idx ON agent_dynamic_context USING hnsw (embedding vector_l2_ops) WHERE embedding IS NOT NULL;

-- Create the table for knowledge base document chunks
CREATE TABLE IF NOT EXISTS knowledge_base_document_chunks (
    document_id BIGINT NOT NULL,
    knowledge_base_id BIGINT NOT NULL,
    deployment_id BIGINT NOT NULL,
    chunk_index INT NOT NULL,
    content TEXT NOT NULL,
    embedding vector(768) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fk_document
      FOREIGN KEY(document_id)
      REFERENCES ai_knowledge_base_documents(id)
      ON DELETE CASCADE,
    CONSTRAINT fk_knowledge_base
      FOREIGN KEY(knowledge_base_id)
      REFERENCES ai_knowledge_bases(id)
      ON DELETE CASCADE,
    PRIMARY KEY (document_id, chunk_index)
);

-- Create indexes for document chunks
DROP INDEX IF EXISTS idx_kb_doc_chunks_doc_id;
CREATE INDEX idx_kb_doc_chunks_doc_id ON knowledge_base_document_chunks(document_id);

DROP INDEX IF EXISTS idx_kb_doc_chunks_kb_id;
CREATE INDEX idx_kb_doc_chunks_kb_id ON knowledge_base_document_chunks(knowledge_base_id);

DROP INDEX IF EXISTS kb_doc_chunks_embedding_idx;
CREATE INDEX kb_doc_chunks_embedding_idx ON knowledge_base_document_chunks USING hnsw (embedding vector_l2_ops);
