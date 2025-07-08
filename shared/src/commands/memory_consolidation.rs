use crate::{
    commands::{Command, GenerateEmbeddingCommand},
    error::AppError,
    models::{ConversationRecord, ConsolidationCandidate},
    state::AppState,
};
use pgvector::Vector;

/// Find memories that should be consolidated
pub struct FindConsolidationCandidatesCommand {
    pub context_id: Option<i64>,
    pub similarity_threshold: f64,
}

impl Command for FindConsolidationCandidatesCommand {
    type Output = Vec<ConsolidationCandidate>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let _base_query = if let Some(ctx_id) = self.context_id {
            format!("WHERE creation_context_id = {}", ctx_id)
        } else {
            String::new()
        };

        // Find groups of similar memories
        let similar_groups = sqlx::query!(
            r#"
            WITH memory_pairs AS (
                SELECT 
                    m1.id as id1,
                    m2.id as id2,
                    m1.content as content1,
                    m2.content as content2,
                    m1.memory_category as category1,
                    m2.memory_category as category2,
                    1 - (m1.embedding <=> m2.embedding) as similarity
                FROM memories m1
                CROSS JOIN memories m2
                WHERE m1.id < m2.id
                  AND 1 - (m1.embedding <=> m2.embedding) > $1
                  AND m1.memory_category = m2.memory_category
            )
            SELECT 
                id1, id2, content1, content2, category1, similarity
            FROM memory_pairs
            ORDER BY similarity DESC
            LIMIT 50
            "#,
            self.similarity_threshold
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        // Group similar memories
        let mut groups: std::collections::HashMap<i64, Vec<(i64, f64, String)>> = std::collections::HashMap::new();
        
        for pair in similar_groups {
            groups.entry(pair.id1)
                .or_insert_with(Vec::new)
                .push((pair.id2, pair.similarity.unwrap_or(0.0), pair.content2));
        }

        // Create consolidation candidates
        let mut candidates = Vec::new();
        
        for (primary_id, similar) in groups {
            if similar.len() >= 2 {  // Only consolidate if 3+ similar memories
                let primary_content = sqlx::query_scalar!(
                    "SELECT content FROM memories WHERE id = $1",
                    primary_id
                )
                .fetch_one(&app_state.db_pool)
                .await?;

                let similar_ids: Vec<i64> = similar.iter().map(|(id, _, _)| *id).collect();
                let similarity_scores: Vec<f64> = similar.iter().map(|(_, score, _)| *score).collect();
                
                // Generate suggested consolidated content
                let all_contents = std::iter::once(primary_content.clone())
                    .chain(similar.iter().map(|(_, _, content)| content.clone()))
                    .collect::<Vec<_>>();
                
                let suggested_content = generate_consolidated_content(&all_contents);
                
                candidates.push(ConsolidationCandidate {
                    primary_id,
                    similar_ids,
                    similarity_scores,
                    suggested_content,
                    suggested_category: "semantic".to_string(), // Could be smarter
                });
            }
        }

        Ok(candidates)
    }
}

/// Consolidate a group of memories
pub struct ConsolidateMemoriesCommand {
    pub candidate: ConsolidationCandidate,
}

impl Command for ConsolidateMemoriesCommand {
    type Output = i64;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Begin transaction
        let mut tx = app_state.db_pool.begin().await?;

        // Get all memories to consolidate
        let all_ids = std::iter::once(self.candidate.primary_id)
            .chain(self.candidate.similar_ids.iter().cloned())
            .collect::<Vec<_>>();

        // Sum up important metrics
        let metrics = sqlx::query!(
            r#"
            SELECT 
                SUM(access_count) as total_access_count,
                SUM(citation_count) as total_citation_count,
                MAX(learning_confidence) as max_confidence,
                MAX(cross_context_value) as max_cross_value
            FROM memories
            WHERE id = ANY($1)
            "#,
            &all_ids
        )
        .fetch_one(&mut *tx)
        .await?;

        // Generate embedding for consolidated content
        let embedding = GenerateEmbeddingCommand::new(self.candidate.suggested_content.clone())
            .execute(app_state)
            .await?;

        // Create new consolidated memory
        let new_memory_id = app_state.sf.next_id()? as i64;
        let embedding_vector = Vector::from(embedding);
        
        sqlx::query(
            r#"
            INSERT INTO memories (
                id, content, embedding, memory_category,
                base_temporal_score, access_count,
                first_accessed_at, last_accessed_at,
                citation_count, cross_context_value, learning_confidence,
                relevance_score, usefulness_score,
                creation_context_id, last_reinforced_at,
                semantic_centrality, uniqueness_score,
                compression_level, compressed_content,
                context_decay_profile
            ) VALUES (
                $1, $2, $3, $4,
                0.9, $5,
                NOW(), NOW(),
                $6, $7, $8,
                0.0, 0.0,
                NULL, NOW(),
                0.8, 0.9,
                0, NULL,
                '{}'::jsonb
            )
            "#
        )
        .bind(new_memory_id)
        .bind(&self.candidate.suggested_content)
        .bind(embedding_vector)
        .bind(&self.candidate.suggested_category)
        .bind(metrics.total_access_count.unwrap_or(0) as i32)
        .bind(metrics.total_citation_count.unwrap_or(0) as i32)
        .bind(metrics.max_cross_value.unwrap_or(0.5))
        .bind(metrics.max_confidence.unwrap_or(0.7))
        .execute(&mut *tx)
        .await?;

        // Archive old memories (soft delete by setting compression_level = 2)
        sqlx::query!(
            r#"
            UPDATE memories 
            SET compression_level = 2,
                compressed_content = $1,
                updated_at = NOW()
            WHERE id = ANY($2)
            "#,
            format!("Consolidated into memory {}", new_memory_id),
            &all_ids
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        tracing::info!("Consolidated {} memories into memory {}", all_ids.len(), new_memory_id);
        Ok(new_memory_id)
    }
}

/// Promote highly-cited conversations to memories
pub struct PromoteConversationsToMemoriesCommand {
    pub context_id: i64,
    pub citation_threshold: i32,
}

impl Command for PromoteConversationsToMemoriesCommand {
    type Output = Vec<i64>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Find conversations worth promoting
        let conversations = sqlx::query_as::<_, ConversationRecord>(
            r#"
            SELECT *
            FROM conversations
            WHERE context_id = $1
              AND citation_count >= $2
              AND message_type = 'agent_response'
              AND NOT EXISTS (
                  SELECT 1 FROM memories 
                  WHERE content = conversations.content
              )
            ORDER BY citation_count DESC
            LIMIT 10
            "#
        )
        .bind(self.context_id)
        .bind(self.citation_threshold)
        .fetch_all(&app_state.db_pool)
        .await?;

        let mut promoted_ids = Vec::new();

        for conv in conversations {
            // Create memory from conversation
            let memory_id = app_state.sf.next_id()? as i64;
            
            sqlx::query(
                r#"
                INSERT INTO memories (
                    id, content, embedding, memory_category,
                    base_temporal_score, access_count,
                    first_accessed_at, last_accessed_at,
                    citation_count, cross_context_value, learning_confidence,
                    relevance_score, usefulness_score,
                    creation_context_id, last_reinforced_at,
                    semantic_centrality, uniqueness_score,
                    compression_level, compressed_content,
                    context_decay_profile
                ) VALUES (
                    $1, $2, $3, 'episodic',
                    $4, $5,
                    $6, $7,
                    $8, $9, 0.8,
                    $10, $11,
                    $12, NOW(),
                    0.5, 0.7,
                    0, NULL,
                    '{}'::jsonb
                )
                "#
            )
            .bind(memory_id)
            .bind(&conv.content)
            .bind(conv.embedding.clone())
            .bind(conv.base_temporal_score)
            .bind(conv.access_count)
            .bind(conv.first_accessed_at)
            .bind(conv.last_accessed_at)
            .bind(conv.citation_count)
            .bind(conv.usefulness_score)
            .bind(conv.relevance_score)
            .bind(conv.usefulness_score)
            .bind(self.context_id)
            .execute(&app_state.db_pool)
            .await?;

            promoted_ids.push(memory_id);
            
            tracing::info!(
                "Promoted conversation {} to memory {} (citations: {})",
                conv.id, memory_id, conv.citation_count
            );
        }

        Ok(promoted_ids)
    }
}

/// Check if consolidation is needed
pub struct CheckConsolidationNeededQuery {
    pub context_id: Option<i64>,
}

impl crate::queries::Query for CheckConsolidationNeededQuery {
    type Output = bool;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) as "count!"
            FROM memories m1
            CROSS JOIN memories m2
            WHERE m1.id < m2.id
              AND 1 - (m1.embedding <=> m2.embedding) > 0.85
              AND m1.memory_category = m2.memory_category
              AND m1.compression_level = 0
              AND m2.compression_level = 0
            LIMIT 10
            "#
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(count > 5)
    }
}

/// Simple content consolidation
fn generate_consolidated_content(contents: &[String]) -> String {
    // For now, simple concatenation with deduplication
    // In production, this could use LLM to intelligently merge
    
    let mut combined = String::new();
    let mut seen_sentences = std::collections::HashSet::new();
    
    for content in contents {
        for sentence in content.split(". ") {
            let normalized = sentence.trim().to_lowercase();
            if !seen_sentences.contains(&normalized) && !normalized.is_empty() {
                if !combined.is_empty() {
                    combined.push_str(". ");
                }
                combined.push_str(sentence.trim());
                seen_sentences.insert(normalized);
            }
        }
    }
    
    if !combined.ends_with('.') {
        combined.push('.');
    }
    
    combined
}