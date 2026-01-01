use super::core::AgentExecutor;

use commands::{Command, GenerateEmbeddingsCommand, UpdateMemoryAccessCommand};
use common::error::AppError;
use dto::json::agent_executor::MemoryLoadingDirective;
use dto::json::agent_executor::MemoryScope;
use models::{ImmediateContext, MemoryRecord};
use queries::{
    GetAgentMemoriesQuery, GetLLMConversationHistoryQuery, GetMRUMemoriesQuery,
    GetSessionMemoriesQuery, Query, SearchMemoriesWithDecayQuery,
};

impl AgentExecutor {
    pub async fn get_immediate_context(&self) -> Result<ImmediateContext, AppError> {
        let (mru_memories, recent_conversations) =
            tokio::join!(self.get_mru_memories(20), self.get_recent_conversations());

        Ok(ImmediateContext {
            memories: mru_memories?,
            conversations: recent_conversations?,
        })
    }

    pub(super) async fn load_memories_with_directive(
        &mut self,
        directive: MemoryLoadingDirective,
    ) -> Result<(), AppError> {
        tracing::info!(
            "Loading memories with scope: {:?}, focus: {}, categories: {:?}",
            directive.scope,
            directive.focus,
            directive.categories
        );

        let embedding = if !directive.focus.is_empty() {
            match GenerateEmbeddingsCommand::new(vec![directive.focus.clone()])
                .with_task_type("RETRIEVAL_QUERY".to_string())
                .execute(&self.app_state)
                .await
            {
                Ok(embeddings) if !embeddings.is_empty() => Some(embeddings[0].clone()),
                _ => None,
            }
        } else {
            None
        };

        let limit = match directive.depth {
            dto::json::agent_executor::SearchDepth::Shallow => 20,
            dto::json::agent_executor::SearchDepth::Moderate => 50,
            dto::json::agent_executor::SearchDepth::Deep => 100,
        };

        let memories = match directive.scope {
            MemoryScope::CurrentSession => {
                self.load_session_memories(&directive, embedding, limit)
                    .await?
            }
            MemoryScope::CrossSession => {
                self.load_agent_patterns(&directive, embedding, limit)
                    .await?
            }
            MemoryScope::Universal => {
                self.load_all_relevant_memories(&directive, embedding, limit)
                    .await?
            }
        };

        tracing::info!("Loaded {} memories", memories.len());

        for memory in &memories {
            self.loaded_memory_ids.insert(memory.id);
        }

        self.memories = memories;

        Ok(())
    }

    async fn get_mru_memories(&self, limit: usize) -> Result<Vec<MemoryRecord>, AppError> {
        GetMRUMemoriesQuery {
            context_id: self.context_id,
            limit: limit as i64,
        }
        .execute(&self.app_state)
        .await
    }

    pub(super) async fn reinforce_used_memories(&self) -> Result<(), AppError> {
        if self.loaded_memory_ids.is_empty() {
            return Ok(());
        }

        tracing::info!(
            "Reinforcing {} loaded memories",
            self.loaded_memory_ids.len()
        );

        for memory_id in &self.loaded_memory_ids {
            let command = UpdateMemoryAccessCommand {
                memory_id: *memory_id,
            };
            if let Err(e) = command.execute(&self.app_state).await {
                tracing::warn!("Failed to reinforce memory {}: {}", memory_id, e);
            }
        }

        Ok(())
    }

    async fn load_session_memories(
        &self,
        directive: &MemoryLoadingDirective,
        embedding: Option<Vec<f32>>,
        limit: i64,
    ) -> Result<Vec<MemoryRecord>, AppError> {
        if let Some(embed) = embedding {
            let results = SearchMemoriesWithDecayQuery {
                query_embedding: embed,
                limit,
                context_id: Some(self.context_id),
                agent_id: None,
                categories: Some(directive.categories.clone()),
            }
            .execute(&self.app_state)
            .await?;

            Ok(results.into_iter().map(|r| r.memory).collect())
        } else {
            GetSessionMemoriesQuery {
                context_id: self.context_id,
                categories: Some(directive.categories.clone()),
                limit,
            }
            .execute(&self.app_state)
            .await
        }
    }

    async fn load_agent_patterns(
        &self,
        directive: &MemoryLoadingDirective,
        embedding: Option<Vec<f32>>,
        limit: i64,
    ) -> Result<Vec<MemoryRecord>, AppError> {
        if let Some(embed) = embedding {
            let results = SearchMemoriesWithDecayQuery {
                query_embedding: embed,
                limit,
                context_id: None,
                agent_id: Some(self.agent.id),
                categories: Some(directive.categories.clone()),
            }
            .execute(&self.app_state)
            .await?;

            Ok(results.into_iter().map(|r| r.memory).collect())
        } else {
            GetAgentMemoriesQuery {
                agent_id: self.agent.id,
                categories: Some(directive.categories.clone()),
                limit,
            }
            .execute(&self.app_state)
            .await
        }
    }

    async fn load_all_relevant_memories(
        &self,
        directive: &MemoryLoadingDirective,
        embedding: Option<Vec<f32>>,
        limit: i64,
    ) -> Result<Vec<MemoryRecord>, AppError> {
        if let Some(embed) = embedding {
            let results = SearchMemoriesWithDecayQuery {
                query_embedding: embed,
                limit,
                context_id: Some(self.context_id),
                agent_id: Some(self.agent.id),
                categories: Some(directive.categories.clone()),
            }
            .execute(&self.app_state)
            .await?;

            Ok(results.into_iter().map(|r| r.memory).collect())
        } else {
            let session_memories = self
                .load_session_memories(directive, None, limit / 2)
                .await?;
            let agent_memories = self.load_agent_patterns(directive, None, limit / 2).await?;

            let mut all_memories = session_memories;
            let existing_ids: std::collections::HashSet<i64> =
                all_memories.iter().map(|m| m.id).collect();

            for memory in agent_memories {
                if !existing_ids.contains(&memory.id) {
                    all_memories.push(memory);
                }
            }

            Ok(all_memories)
        }
    }

    pub(super) async fn get_recent_conversations(
        &self,
    ) -> Result<Vec<models::ConversationRecord>, AppError> {
        let records = GetLLMConversationHistoryQuery {
            context_id: self.context_id,
        }
        .execute(&self.app_state)
        .await?;

        Ok(records)
    }
}
