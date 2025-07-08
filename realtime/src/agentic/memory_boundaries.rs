use shared::error::AppError;
use shared::state::AppState;
use shared::models::MemoryBoundaries;
use shared::commands::{
    CreateMemoryBoundariesCommand, CompressOldConversationsCommand,
    EvictLowScoreItemsCommand, EnforceConversationLimitCommand,
    EnforceMemoryLimitsCommand, CheckCleanupNeededQuery, Command,
};
use shared::queries::{GetAllMemoryBoundariesQuery, GetMemoryBoundariesQuery, Query};
use tracing::{info, warn};
use tokio::time::interval;
use std::time::Duration as StdDuration;
use std::sync::Arc;

/// Manages memory boundaries and cleanup operations
pub struct MemoryBoundaryManager {
    app_state: AppState,
}

impl MemoryBoundaryManager {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    /// Start the background cleanup task
    pub fn start_background_cleanup(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut ticker = interval(StdDuration::from_secs(3600)); // Run every hour
            loop {
                ticker.tick().await;
                if let Err(e) = self.run_cleanup_cycle().await {
                    warn!("Memory cleanup cycle failed: {}", e);
                }
            }
        });
    }

    /// Run a complete cleanup cycle across all contexts
    async fn run_cleanup_cycle(&self) -> Result<(), AppError> {
        info!("Starting memory cleanup cycle");
        
        // Get all active contexts with boundaries
        let boundaries = self.get_all_context_boundaries().await?;
        
        for boundary in boundaries {
            let context_id = boundary.context_id;
            if let Err(e) = self.cleanup_context(boundary).await {
                warn!("Failed to cleanup context {}: {}", context_id, e);
            }
        }
        
        info!("Memory cleanup cycle completed");
        Ok(())
    }

    /// Cleanup a specific context based on its boundaries
    async fn cleanup_context(&self, boundary: MemoryBoundaries) -> Result<(), AppError> {
        // 1. Compress old conversations
        let compressed = CompressOldConversationsCommand {
            context_id: boundary.context_id,
            threshold_days: boundary.compression_threshold_days,
        }
        .execute(&self.app_state)
        .await?;
        
        if compressed > 0 {
            info!("Compressed {} conversations for context {}", compressed, boundary.context_id);
        }
        
        // 2. Evict low-score memories
        let evicted = EvictLowScoreItemsCommand {
            context_id: boundary.context_id,
            threshold: boundary.eviction_threshold_score,
        }
        .execute(&self.app_state)
        .await?;
        
        if evicted > 0 {
            info!("Evicted {} low-score items for context {}", evicted, boundary.context_id);
        }
        
        // 3. Enforce max limits
        EnforceConversationLimitCommand {
            context_id: boundary.context_id,
            max_conversations: boundary.max_conversations,
        }
        .execute(&self.app_state)
        .await?;
        
        EnforceMemoryLimitsCommand {
            context_id: boundary.context_id,
            limits: boundary.max_memories_per_category,
        }
        .execute(&self.app_state)
        .await?;
        
        Ok(())
    }

    /// Get boundaries for all contexts
    async fn get_all_context_boundaries(&self) -> Result<Vec<MemoryBoundaries>, AppError> {
        GetAllMemoryBoundariesQuery
            .execute(&self.app_state)
            .await
    }

    /// Get or create boundaries for a context
    pub async fn get_or_create_boundaries(&self, context_id: i64) -> Result<MemoryBoundaries, AppError> {
        // Try to get existing boundaries
        if let Ok(boundaries) = self.get_context_boundaries(context_id).await {
            return Ok(boundaries);
        }
        
        // Create default boundaries
        CreateMemoryBoundariesCommand {
            context_id,
        }
        .execute(&self.app_state)
        .await
    }

    /// Get boundaries for a specific context
    async fn get_context_boundaries(&self, context_id: i64) -> Result<MemoryBoundaries, AppError> {
        GetMemoryBoundariesQuery {
            context_id,
        }
        .execute(&self.app_state)
        .await
    }

    /// Check if cleanup is needed for a context
    pub async fn needs_cleanup(&self, context_id: i64) -> Result<bool, AppError> {
        CheckCleanupNeededQuery {
            context_id,
        }
        .execute(&self.app_state)
        .await
    }
}