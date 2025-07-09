use shared::error::AppError;
use shared::models::{
    ConversationRecord, MemoryRecordV2, ImmediateContext, RefinedContext,
    EnhancedCitation, MemoryWithScore,
};
use shared::state::AppState;
use shared::commands::{UpdateCitationMetricsCommand, CitationType, Command};
use shared::queries::{
    GetMRUMemoriesQuery, GetRecentConversationsQuery, Query,
    SearchMemoriesWithDecayQuery, SearchConversationsQuery,
    UpdateMemoryAccessCommand,
};

/// Manages decay-based memory retrieval with 2-phase approach
pub struct DecayManager {
    app_state: AppState,
}

impl DecayManager {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    /// Phase 1: Get immediate context (no computation, pure SQL ordering)
    /// This should complete in ~5-10ms
    pub async fn get_immediate_context(
        &self,
        context_id: i64,
    ) -> Result<ImmediateContext, AppError> {
        // Run both queries in parallel for maximum speed
        let (mru_memories, recent_conversations) = tokio::join!(
            self.get_mru_memories(20),
            self.get_recent_conversations(context_id, 10)
        );

        Ok(ImmediateContext {
            memories: mru_memories?,
            conversations: recent_conversations?,
        })
    }

    /// Get Most Recently Used memories (no filtering, just ORDER BY)
    async fn get_mru_memories(&self, limit: usize) -> Result<Vec<MemoryRecordV2>, AppError> {
        GetMRUMemoriesQuery {
            limit: limit as i64,
        }
        .execute(&self.app_state)
        .await
    }

    /// Get recent conversations (10 most recent user messages + agent responses)
    async fn get_recent_conversations(
        &self,
        context_id: i64,
        limit: usize,
    ) -> Result<Vec<ConversationRecord>, AppError> {
        let mut records = GetRecentConversationsQuery {
            context_id,
            limit: limit as i64,
        }
        .execute(&self.app_state)
        .await?;

        // Reverse to get chronological order (oldest first)
        records.reverse();

        Ok(records)
    }

    /// Update access metrics when a memory is accessed (fire-and-forget)
    pub fn record_memory_access_async(&self, memory_id: i64) {
        let app_state = self.app_state.clone();
        
        tokio::spawn(async move {
            let _ = UpdateMemoryAccessCommand {
                memory_id,
            }
            .execute(&app_state)
            .await;
        });
    }


    /// Phase 2: Refine context based on reasoning embedding
    pub async fn refine_context_from_reasoning(
        &self,
        reasoning_embedding: &[f32],
        context_id: i64,
        max_results: usize,
    ) -> Result<RefinedContext, AppError> {
        // Search memories with embedding similarity and decay scores
        let memory_results = SearchMemoriesWithDecayQuery {
            query_embedding: reasoning_embedding.to_vec(),
            limit: max_results as i64,
        }
        .execute(&self.app_state)
        .await?;
        
        // Search conversations without decay scores
        let conversation_results = SearchConversationsQuery {
            context_id,
            limit: max_results as i64,
        }
        .execute(&self.app_state)
        .await?;
        
        Ok(RefinedContext {
            relevant_memories: memory_results.into_iter()
                .map(|m| MemoryWithScore {
                    memory: m.memory,
                    similarity_score: m.similarity_score,
                    decay_adjusted_score: m.decay_adjusted_score,
                })
                .collect(),
            relevant_conversations: conversation_results,
        })
    }

    /// Update decay scores based on citation usage
    pub async fn update_decay_from_citations(
        &self,
        citations: &[EnhancedCitation],
    ) -> Result<(), AppError> {
        for citation in citations {
            let relevance_delta = citation.relevance_score * 0.1; // Max 0.09 per citation
            let usefulness_delta = citation.usefulness_score * 0.1;

            let citation_type = match citation.item_type {
                shared::models::CitationType::Memory => CitationType::Memory,
                _ => continue, // Skip other types
            };

            UpdateCitationMetricsCommand {
                item_id: citation.item_id,
                item_type: citation_type,
                relevance_delta,
                usefulness_delta,
            }
            .execute(&self.app_state)
            .await?;
        }

        Ok(())
    }
}