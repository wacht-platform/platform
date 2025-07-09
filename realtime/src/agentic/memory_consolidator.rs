use shared::error::AppError;
use shared::state::AppState;
use shared::models::ConsolidationCandidate;
use shared::commands::{
    FindConsolidationCandidatesCommand, ConsolidateMemoriesCommand,
    CheckConsolidationNeededQuery,
    Command,
};
use shared::queries::Query;

/// Consolidates similar memories
pub struct MemoryConsolidator {
    app_state: AppState,
}

impl MemoryConsolidator {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    /// Find memories that should be consolidated
    pub async fn find_consolidation_candidates(
        &self,
        context_id: Option<i64>,
        similarity_threshold: f64
    ) -> Result<Vec<ConsolidationCandidate>, AppError> {
        FindConsolidationCandidatesCommand {
            context_id,
            similarity_threshold,
        }
        .execute(&self.app_state)
        .await
    }

    /// Consolidate a group of memories
    pub async fn consolidate_memories(
        &self,
        candidate: ConsolidationCandidate
    ) -> Result<i64, AppError> {
        ConsolidateMemoriesCommand {
            candidate,
        }
        .execute(&self.app_state)
        .await
    }


    /// Check if consolidation is needed
    pub async fn needs_consolidation(&self, context_id: Option<i64>) -> Result<bool, AppError> {
        CheckConsolidationNeededQuery {
            context_id,
        }
        .execute(&self.app_state)
        .await
    }
}