-- Debug script to test hybrid search
-- Run this manually to check if search is working

-- Check if documents exist for Niroj
SELECT 
    d.id,
    d.title,
    d.description,
    COUNT(c.chunk_index) as chunk_count
FROM ai_knowledge_base_documents d
LEFT JOIN knowledge_base_document_chunks c ON d.id = c.document_id
WHERE d.knowledge_base_id = 22792507398037315
    AND (d.title ILIKE '%niroj%' OR d.description ILIKE '%niroj%' OR c.content ILIKE '%niroj%')
GROUP BY d.id, d.title, d.description;

-- Check if search vectors are populated
SELECT 
    COUNT(*) as total_chunks,
    COUNT(search_vector) as chunks_with_vectors,
    COUNT(CASE WHEN search_vector IS NULL THEN 1 END) as chunks_without_vectors
FROM knowledge_base_document_chunks
WHERE knowledge_base_id = 22792507398037315;

-- Test text search directly
SELECT 
    document_id,
    chunk_index,
    LEFT(content, 100) as content_preview,
    ts_rank(search_vector, plainto_tsquery('english', 'Niroj')) as text_rank
FROM knowledge_base_document_chunks
WHERE knowledge_base_id = 22792507398037315
    AND search_vector @@ plainto_tsquery('english', 'Niroj')
ORDER BY text_rank DESC
LIMIT 5;

-- Test if the function works with dummy data
SELECT * FROM hybrid_search_kb_enhanced(
    'Niroj performance',
    '[0.1, 0.2, 0.3]'::vector(768),  -- dummy vector
    22792507398037315,
    24003657875857219,  -- deployment_id from logs
    5,
    0.0,  -- min_relevance = 0 to see all results
    0.5,
    0.5
);

-- Check actual content that mentions Niroj
SELECT 
    document_id,
    chunk_index,
    content
FROM knowledge_base_document_chunks
WHERE knowledge_base_id = 22792507398037315
    AND content ILIKE '%niroj%'
LIMIT 3;