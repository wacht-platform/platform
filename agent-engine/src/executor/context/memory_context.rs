use super::core::AgentExecutor;
use common::{error::AppError, get_startup_memories_in_table};
use models::{ImmediateContext, MemoryRecord};
use queries::GetLLMConversationHistoryQuery;

impl AgentExecutor {
    pub async fn get_immediate_context(&self) -> Result<ImmediateContext, AppError> {
        let (mru_memories, recent_conversations) = tokio::join!(
            self.get_startup_memories(50),
            self.get_recent_conversations()
        );

        Ok(ImmediateContext {
            memories: mru_memories?,
            conversations: recent_conversations?,
        })
    }

    async fn get_startup_memories(&self, limit: usize) -> Result<Vec<MemoryRecord>, AppError> {
        let thread = self.ctx.get_thread().await?;
        let Some(table) = self.ctx.get_memory_table().await? else {
            return Ok(Vec::new());
        };

        get_startup_memories_in_table(
            &table,
            self.ctx.agent.deployment_id,
            self.ctx.thread_id,
            thread.actor_id,
            limit,
            self.ctx.provider_keys.embedding_dimension,
        )
        .await
    }

    pub(crate) async fn get_recent_conversations(
        &self,
    ) -> Result<Vec<models::ConversationRecord>, AppError> {
        GetLLMConversationHistoryQuery::new(self.ctx.thread_id)
            .with_board_item_id(self.current_board_item_id())
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
    }
}
