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
use serde_json::Value;

impl AgentExecutor {
    pub async fn get_immediate_context(&self) -> Result<ImmediateContext, AppError> {
        let (mru_memories, recent_conversations) =
            tokio::join!(self.get_mru_memories(20), self.get_recent_conversations());

        let mut conversations = recent_conversations?;

        let mut found_most_recent_execution = false;

        for conv in conversations.iter_mut().rev() {
            if let models::ConversationContent::ActionExecutionResult { task_execution, .. } =
                &mut conv.content
            {
                if !found_most_recent_execution {
                    found_most_recent_execution = true;
                    continue;
                }

                if let Some(ref mut results) = task_execution.actual_result {
                    for result in results.iter_mut() {
                        if let Some(ref mut output) = result.result {
                            let output_str = output.to_string();
                            if output_str.len() > 500 {
                                *output = serde_json::json!({
                                    "truncated": true,
                                    "preview": output_str.chars().take(500).collect::<String>(),
                                    "note": "Historical output truncated to save context."
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(ImmediateContext {
            memories: mru_memories?,
            conversations,
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
                .execute(&self.ctx.app_state)
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
            context_id: self.ctx.context_id,
            limit: limit as i64,
        }
        .execute(&self.ctx.app_state)
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
            if let Err(e) = command.execute(&self.ctx.app_state).await {
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
                context_id: Some(self.ctx.context_id),
                agent_id: None,
                categories: Some(directive.categories.clone()),
            }
            .execute(&self.ctx.app_state)
            .await?;

            Ok(results.into_iter().map(|r| r.memory).collect())
        } else {
            GetSessionMemoriesQuery {
                context_id: self.ctx.context_id,
                categories: Some(directive.categories.clone()),
                limit,
            }
            .execute(&self.ctx.app_state)
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
                agent_id: Some(self.ctx.agent.id),
                categories: Some(directive.categories.clone()),
            }
            .execute(&self.ctx.app_state)
            .await?;

            Ok(results.into_iter().map(|r| r.memory).collect())
        } else {
            GetAgentMemoriesQuery {
                agent_id: self.ctx.agent.id,
                categories: Some(directive.categories.clone()),
                limit,
            }
            .execute(&self.ctx.app_state)
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
                context_id: Some(self.ctx.context_id),
                agent_id: Some(self.ctx.agent.id),
                categories: Some(directive.categories.clone()),
            }
            .execute(&self.ctx.app_state)
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
            context_id: self.ctx.context_id,
        }
        .execute(&self.ctx.app_state)
        .await?;

        let context = self.ctx.get_context().await?;
        let metadata = context.external_resource_metadata.as_ref();
        let inherit_parent_context_id =
            metadata.and_then(|meta| parse_i64_metadata(meta, "inherit_parent_context_id"));
        let inherit_parent_until = metadata
            .and_then(|meta| parse_i64_metadata(meta, "inherit_parent_until_conversation_id"));

        let (Some(parent_context_id), Some(parent_until_id)) =
            (inherit_parent_context_id, inherit_parent_until)
        else {
            return Ok(records);
        };

        if parent_context_id <= 0 || parent_until_id <= 0 {
            return Ok(records);
        }

        let parent_records = GetLLMConversationHistoryQuery::new(parent_context_id)
            .execute(&self.ctx.app_state)
            .await?;

        let mut merged: Vec<models::ConversationRecord> = parent_records
            .into_iter()
            .filter(|conv| conv.id <= parent_until_id)
            .collect();
        merged.extend(records);
        merged.sort_by_key(|conv| conv.id);
        merged.dedup_by_key(|conv| conv.id);

        Ok(merged)
    }
}

fn parse_i64_metadata(metadata: &Value, key: &str) -> Option<i64> {
    let value = metadata.get(key)?;
    if let Some(number) = value.as_i64() {
        return Some(number);
    }
    value.as_str()?.trim().parse::<i64>().ok()
}
